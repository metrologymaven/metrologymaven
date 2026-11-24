//! `fid-design comp-check --root <components dir | spec file> --package <dir> [--json]` (spec
//! node `UX.COMP.SCHEMA`, REQ-UX.COMP.SCHEMA-001 .. -006). Tooling, nearest class R0; D0
//! (`BTreeMap`/`BTreeSet` only, sorted walk and findings, no clock, no network).
//!
//! # `component-spec@1`
//!
//! A component spec is a Markdown file whose first line opens a fenced `toml` block (the front
//! block), closed by a line holding only the closing fence. The prose after it holds the
//! sections `## Anatomy`, `## Behaviour`, `## Keyboard`, `## Accessibility` and
//! `## Do and don't` (each as its own heading line). `design/components/TEMPLATE.md` is a
//! commented, checked example of the whole format.
//!
//! Front block keys (a closed set; an unknown key is `COMP-SCHEMA`):
//! - `format = "component-spec@1"`, `id = "comp.<name>"` (`<name>` is `[a-z][a-z0-9-]*`, unique
//!   across the root), `title`.
//! - `[[parts]]`: `name` (`[a-z][a-z0-9-]*`, unique), optional `actionable` (bool, default
//!   false) and `description`. A spec is *interactive* when a part is actionable.
//! - `[states]`: all eight of `default`, `hover`, `focus`, `active`, `disabled`, `read-only`,
//!   `error`, `loading`, each `{ applies = true }` or `{ applies = false, reason = ".." }`.
//!   `default` always applies; `focus` applies in an interactive spec.
//! - `[tokens.<part>.<state>]`: `<property> = "<token name>"`, for declared parts and applying
//!   states; every part has `default` tokens.
//! - `[density.<part>.compact]` and `[density.<part>.comfortable]`: `<property> = "<token>"`,
//!   both modes with the same properties, at least one part.
//! - `[[keyboard]]`: `key`, `context`, `action` (`[a-z][a-z0-9-]*`); `(key, context)` unique;
//!   at least one entry in an interactive spec (an empty array `keyboard = []` otherwise).
//! - `[[pointer_actions]]` (optional): `gesture`, `part` (an actionable part), `action`.
//! - `[a11y]`: `role` (from [`ROLES`]), `name_source` (text), `states` (from [`ARIA_STATES`];
//!   must hold `disabled` when the disabled state applies).
//! - `[focus]`: `token`, `against` (the surfaces the indicator is drawn on), `width_px`
//!   (integer), `offset_px` (integer).
//! - `[disabled_reason]`: `shown_as` (`visible` or `focus-reachable`), `part` (the anatomy part
//!   that carries the text), optional `note`. Required when the disabled state applies.
//!
//! # Findings (exit 1)
//!
//! - `COMP-SCHEMA <file>`: no front block, invalid TOML, wrong format, a missing or malformed
//!   key, an unknown key, a missing prose section, a duplicate id, an empty root.
//! - `COMP-TOKEN-UNKNOWN <token>`: a token named anywhere in the front block (tokens, density,
//!   focus) does not resolve in the package (`fid_design_pkg::Package::resolve`: unknown name,
//!   alias cycle, too many hops, deprecated without replacement).
//! - `COMP-STATE-MISSING <state>`: one of the eight states is not listed.
//! - `COMP-POINTER-ONLY <action>`: a pointer action has no keyboard entry with the same action.
//! - `COMP-FOCUS <fg>/<bg>`: a focus pair (`focus.token` on an `against` surface) is not declared
//!   `non-text` in the package's `tokens/pairs.json` (`fid-design-pairs@1`, read as written, no
//!   alias following). `COMP-FOCUS <file>`: no `focus` table, `width_px` below
//!   [`MIN_FOCUS_WIDTH_PX`], or an interactive spec whose focus state does not apply.
//! - `COMP-DISABLED-REASON <file>`: the disabled state applies and no valid `disabled_reason`
//!   shows the reason as visible or focus-reachable text, or a given `disabled_reason` is invalid.
//! - `COMP-ROLE <role>`: `a11y.role` is missing or not in [`ROLES`].
//! - `COMP-DENSITY <file>`: no density table, an unknown part, a missing mode or mismatched
//!   properties between the modes.
//!
//! Package structure problems are `pkg-check`'s job and are not repeated here.
//!
//! Exit codes: 0 no findings, 1 findings, 2 unreadable root, spec, package or pairs file, or
//! a limit exceeded.

use crate::finding::{findings_text, findings_value, read_text, Finding};
use crate::tokens::{PAIRS_FORMAT, PAIR_TAGS};
use fid_canon::CValue;
use fid_design_pkg::{is_valid_name, Package};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Front-block format ID.
pub const SPEC_FORMAT: &str = "component-spec@1";
/// Report format ID.
pub const REPORT_FORMAT: &str = "fid-design-comp-check@1";
/// Contrast-pair declarations, relative to the package root.
pub const PAIRS_FILE: &str = "tokens/pairs.json";

pub const SCHEMA: &str = "COMP-SCHEMA";
pub const TOKEN_UNKNOWN: &str = "COMP-TOKEN-UNKNOWN";
pub const STATE_MISSING: &str = "COMP-STATE-MISSING";
pub const POINTER_ONLY: &str = "COMP-POINTER-ONLY";
pub const FOCUS: &str = "COMP-FOCUS";
pub const DISABLED_REASON: &str = "COMP-DISABLED-REASON";
pub const ROLE: &str = "COMP-ROLE";
pub const DENSITY: &str = "COMP-DENSITY";

/// Largest spec file read.
pub const MAX_SPEC_BYTES: u64 = 256 * 1024;
/// Largest `tokens/pairs.json` read.
pub const MAX_PAIRS_BYTES: u64 = 1024 * 1024;
/// Most spec files under one root.
pub const MAX_SPECS: usize = 512;
/// Most entries in any one array or table of the front block.
pub const MAX_ITEMS: usize = 256;
/// Longest free-text value, in characters.
pub const MAX_TEXT_CHARS: usize = 1000;
/// Thinnest focus indicator accepted (REQ-UX.COMP.SCHEMA-004; WCAG 2.2 SC 2.4.13 guidance
/// [unverified that 2 px matches the SC's area formula]).
pub const MIN_FOCUS_WIDTH_PX: i64 = 2;
/// Largest focus width or absolute offset accepted.
pub const MAX_FOCUS_PX: i64 = 64;

/// The eight states, in spec order.
pub const STATES: [&str; 8] = ["default", "hover", "focus", "active", "disabled", "read-only", "error", "loading"];
/// The prose sections every spec holds, as `## <name>` heading lines.
pub const SECTIONS: [&str; 5] = ["Anatomy", "Behaviour", "Keyboard", "Accessibility", "Do and don't"];
/// How a disabled reason may be shown (REQ-UX.COMP-008: never by pointer hover alone).
pub const REASON_MODES: [&str; 2] = ["focus-reachable", "visible"];
/// Density modes.
pub const DENSITY_MODES: [&str; 2] = ["comfortable", "compact"];

/// The closed role list (REQ-UX.COMP.SCHEMA-006): the non-abstract role names of WAI-ARIA 1.2
/// without the deprecated `directory` [unverified that the list matches the Recommendation
/// exactly]. Sorted.
pub const ROLES: [&str; 81] = [
    "alert",
    "alertdialog",
    "application",
    "article",
    "banner",
    "blockquote",
    "button",
    "caption",
    "cell",
    "checkbox",
    "code",
    "columnheader",
    "combobox",
    "complementary",
    "contentinfo",
    "definition",
    "deletion",
    "dialog",
    "document",
    "emphasis",
    "feed",
    "figure",
    "form",
    "generic",
    "grid",
    "gridcell",
    "group",
    "heading",
    "img",
    "insertion",
    "link",
    "list",
    "listbox",
    "listitem",
    "log",
    "main",
    "marquee",
    "math",
    "menu",
    "menubar",
    "menuitem",
    "menuitemcheckbox",
    "menuitemradio",
    "meter",
    "navigation",
    "none",
    "note",
    "option",
    "paragraph",
    "presentation",
    "progressbar",
    "radio",
    "radiogroup",
    "region",
    "row",
    "rowgroup",
    "rowheader",
    "scrollbar",
    "search",
    "searchbox",
    "separator",
    "slider",
    "spinbutton",
    "status",
    "strong",
    "subscript",
    "superscript",
    "switch",
    "tab",
    "table",
    "tablist",
    "tabpanel",
    "term",
    "textbox",
    "time",
    "timer",
    "toolbar",
    "tooltip",
    "tree",
    "treegrid",
    "treeitem",
];

/// WAI-ARIA 1.2 state and property names without the `aria-` prefix and without the deprecated
/// `dropeffect` and `grabbed` [unverified that the list matches the Recommendation exactly].
/// Sorted.
pub const ARIA_STATES: [&str; 46] = [
    "activedescendant",
    "atomic",
    "autocomplete",
    "busy",
    "checked",
    "colcount",
    "colindex",
    "colspan",
    "controls",
    "current",
    "describedby",
    "details",
    "disabled",
    "errormessage",
    "expanded",
    "flowto",
    "haspopup",
    "hidden",
    "invalid",
    "keyshortcuts",
    "label",
    "labelledby",
    "level",
    "live",
    "modal",
    "multiline",
    "multiselectable",
    "orientation",
    "owns",
    "placeholder",
    "posinset",
    "pressed",
    "readonly",
    "relevant",
    "required",
    "roledescription",
    "rowcount",
    "rowindex",
    "rowspan",
    "selected",
    "setsize",
    "sort",
    "valuemax",
    "valuemin",
    "valuenow",
    "valuetext",
];

const TOP_KEYS: [&str; 12] = [
    "a11y",
    "density",
    "disabled_reason",
    "focus",
    "format",
    "id",
    "keyboard",
    "parts",
    "pointer_actions",
    "states",
    "title",
    "tokens",
];

/// Declared contrast pairs: `(fg, bg)` to tag.
pub type Pairs = BTreeMap<(String, String), String>;

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
    pub findings: Vec<Finding>,
}

/// Checks every `*.md` file directly under `root` (or the one file `root`) against the package
/// at `package`. `Err` is an unreadable input (exit 2).
pub fn check(root: &Path, package: &Path) -> Result<Report, String> {
    let pkg = Package::load(package).map_err(|e| format!("cannot load --package {}: {e}", package.display()))?;
    let pairs = load_pairs(package)?;
    let mut f = Vec::new();
    let files = spec_files(root, &mut f)?;
    let mut specs = Vec::new();
    for (name, path) in &files {
        let text = read_text(path, MAX_SPEC_BYTES)?;
        let id = check_text(name, &text, &pkg, &pairs, &mut f);
        specs.push(SpecSummary { file: name.clone(), id });
    }
    if files.is_empty() {
        f.push(Finding::new(SCHEMA, "--root", "no component spec (*.md) found"));
    }
    let mut ids: BTreeMap<&str, &str> = BTreeMap::new();
    for s in &specs {
        if let Some(id) = &s.id {
            if let Some(first) = ids.insert(id.as_str(), s.file.as_str()) {
                f.push(Finding::new(SCHEMA, &s.file, format!("id {id} is also used by {first}")));
            }
        }
    }
    f.sort();
    f.dedup();
    Ok(Report { specs, findings: f })
}

/// The spec files: `root` itself when it is a file, else the `*.md` entries directly under it,
/// sorted by name. Symbolic links are not followed (a finding); subdirectories are skipped.
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
    let mut out = Vec::new();
    let rd = std::fs::read_dir(root).map_err(|e| format!("cannot read --root {}: {e}", root.display()))?;
    for e in rd {
        let e = e.map_err(|e| format!("cannot read --root {}: {e}", root.display()))?;
        let Some(name) = e.file_name().to_str().map(str::to_string) else {
            f.push(Finding::new(SCHEMA, "--root", "a file name is not valid UTF-8"));
            continue;
        };
        if !name.ends_with(".md") {
            continue;
        }
        let ft = e.file_type().map_err(|err| format!("cannot read {}: {err}", e.path().display()))?;
        if ft.is_symlink() {
            f.push(Finding::new(SCHEMA, &name, "symbolic links are not followed"));
        } else if ft.is_file() {
            out.push((name, e.path()));
            if out.len() > MAX_SPECS {
                return Err(format!("more than {MAX_SPECS} spec files under {}", root.display()));
            }
        }
    }
    out.sort();
    Ok(out)
}

/// Reads `tokens/pairs.json` of the package. The inner `Err` is a content problem (missing
/// file, invalid JSON or shape), reported against every focus pair; the outer `Err` is an
/// unreadable file (exit 2).
pub fn load_pairs(package: &Path) -> Result<Result<Pairs, String>, String> {
    let p = package.join("tokens").join("pairs.json");
    match std::fs::symlink_metadata(&p) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Err(format!("the package has no {PAIRS_FILE}")));
        }
        Err(e) => return Err(format!("cannot read {}: {e}", p.display())),
        Ok(m) if !m.is_file() => return Ok(Err(format!("{PAIRS_FILE} is not a regular file"))),
        Ok(_) => {}
    }
    let text = read_text(&p, MAX_PAIRS_BYTES)?;
    Ok(parse_pairs(&text))
}

/// Parses a `fid-design-pairs@1` document (duplicate JSON keys and duplicate pairs refused).
pub fn parse_pairs(text: &str) -> Result<Pairs, String> {
    let v = fid_canon::parse_with_limits(text.as_bytes(), MAX_PAIRS_BYTES as usize, 8)
        .map_err(|e| format!("{PAIRS_FILE} is not valid JSON: {e}"))?;
    let bad = |why: &str| format!("{PAIRS_FILE}: {why}");
    let CValue::Obj(m) = v else {
        return Err(bad("not a JSON object"));
    };
    if m.len() != 2 || m.get("format") != Some(&CValue::Str(PAIRS_FORMAT.to_string())) {
        return Err(bad(&format!("needs exactly the keys format = {PAIRS_FORMAT:?} and pairs")));
    }
    let Some(CValue::Arr(items)) = m.get("pairs") else {
        return Err(bad("pairs must be an array"));
    };
    let mut out = Pairs::new();
    for (i, item) in items.iter().enumerate() {
        let s = |k: &str| match item {
            CValue::Obj(o) if o.len() == 3 => match o.get(k) {
                Some(CValue::Str(x)) => Some(x.clone()),
                _ => None,
            },
            _ => None,
        };
        let (Some(fg), Some(bg), Some(tag)) = (s("fg"), s("bg"), s("tag")) else {
            return Err(bad(&format!("pair {i} needs exactly the string keys bg, fg, tag")));
        };
        if !PAIR_TAGS.contains(&tag.as_str()) {
            return Err(bad(&format!("pair {i} has tag {tag:?}")));
        }
        if out.insert((fg.clone(), bg.clone()), tag).is_some() {
            return Err(bad(&format!("pair {fg}/{bg} is declared twice")));
        }
    }
    Ok(out)
}

/// Splits a spec into its front block and the prose lines after it.
fn split_front(text: &str) -> Result<(String, Vec<&str>), &'static str> {
    let mut lines = text.lines();
    match lines.next() {
        Some(l) if l.trim_end() == "```toml" => {}
        _ => return Err("the first line must open the fenced toml front block (```toml)"),
    }
    let mut block = String::new();
    for l in lines.by_ref() {
        if l.trim_end() == "```" {
            return Ok((block, lines.collect()));
        }
        block.push_str(l);
        block.push('\n');
    }
    Err("the front block is not closed by a ``` line")
}

/// `[a-z][a-z0-9-]*`, at most 64 bytes.
fn is_ident(s: &str) -> bool {
    let mut b = s.bytes();
    s.len() <= 64
        && b.next().is_some_and(|c| c.is_ascii_lowercase())
        && b.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
}

/// Non-blank text of at most [`MAX_TEXT_CHARS`] characters with no control character.
fn is_text(s: &str) -> bool {
    !s.trim().is_empty() && s.chars().count() <= MAX_TEXT_CHARS && !s.chars().any(char::is_control)
}

fn type_name(v: &toml::Value) -> &'static str {
    v.type_str()
}

struct Ctx<'a> {
    file: &'a str,
    pkg: &'a Package,
    pairs: &'a Result<Pairs, String>,
    f: &'a mut Vec<Finding>,
}

impl Ctx<'_> {
    fn push(&mut self, code: &'static str, subject: &str, detail: impl AsRef<str>) {
        self.f.push(Finding::new(code, subject, format!("{}: {}", self.file, detail.as_ref())));
    }

    fn at_file(&mut self, code: &'static str, detail: impl AsRef<str>) {
        let file = self.file.to_string();
        self.f.push(Finding::new(code, file, detail.as_ref().to_string()));
    }

    fn schema(&mut self, detail: impl AsRef<str>) {
        self.at_file(SCHEMA, detail);
    }

    /// REQ-UX.COMP.SCHEMA-002: the token resolves in the package.
    fn token(&mut self, v: &toml::Value, at: &str) -> Option<String> {
        let toml::Value::String(name) = v else {
            self.schema(format!("{at} must be a token name string, not {}", type_name(v)));
            return None;
        };
        if !is_valid_name(name) || name.len() > 200 {
            self.push(TOKEN_UNKNOWN, name, format!("{at}: not a token name"));
            return None;
        }
        if let Err(e) = self.pkg.resolve(name) {
            self.push(TOKEN_UNKNOWN, name, format!("{at}: does not resolve ({e})"));
        }
        Some(name.clone())
    }

    /// A table with at most [`MAX_ITEMS`] keys, or a finding with `code`.
    fn table<'v>(&mut self, v: &'v toml::Value, at: &str, code: &'static str) -> Option<&'v toml::Table> {
        match v {
            toml::Value::Table(t) if t.len() <= MAX_ITEMS => Some(t),
            toml::Value::Table(_) => {
                self.at_file(code, format!("{at} has more than {MAX_ITEMS} entries"));
                None
            }
            other => {
                self.at_file(code, format!("{at} must be a table, not {}", type_name(other)));
                None
            }
        }
    }

    /// An array of tables with at most [`MAX_ITEMS`] entries, each holding only `allowed` keys
    /// and every key of `required`.
    fn table_array<'v>(
        &mut self,
        v: &'v toml::Value,
        at: &str,
        allowed: &[&str],
        required: &[&str],
    ) -> Vec<&'v toml::Table> {
        let toml::Value::Array(items) = v else {
            self.schema(format!("{at} must be an array of tables, not {}", type_name(v)));
            return Vec::new();
        };
        if items.len() > MAX_ITEMS {
            self.schema(format!("{at} has more than {MAX_ITEMS} entries"));
            return Vec::new();
        }
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

    fn text_field(&mut self, t: &toml::Table, key: &str, at: &str) -> Option<String> {
        match t.get(key) {
            Some(toml::Value::String(s)) if is_text(s) => Some(s.clone()),
            Some(_) => {
                self.schema(format!(
                    "{at}.{key} must be non-blank text of at most {MAX_TEXT_CHARS} characters without control characters"
                ));
                None
            }
            None => None,
        }
    }

    fn ident_field(&mut self, t: &toml::Table, key: &str, at: &str) -> Option<String> {
        match t.get(key) {
            Some(toml::Value::String(s)) if is_ident(s) => Some(s.clone()),
            Some(_) => {
                self.schema(format!("{at}.{key} must match [a-z][a-z0-9-]*"));
                None
            }
            None => None,
        }
    }
}

/// Checks one spec text; returns its `id` when valid. `file` names it in findings.
pub fn check_text(
    file: &str,
    text: &str,
    pkg: &Package,
    pairs: &Result<Pairs, String>,
    f: &mut Vec<Finding>,
) -> Option<String> {
    let mut c = Ctx { file, pkg, pairs, f };
    let (block, prose) = match split_front(text) {
        Ok(x) => x,
        Err(e) => {
            c.schema(e);
            return None;
        }
    };
    for sec in SECTIONS {
        let heading = format!("## {sec}");
        if !prose.iter().any(|l| l.trim_end() == heading) {
            c.schema(format!("missing the prose section `{heading}`"));
        }
    }
    let doc: toml::Table = match block.parse() {
        Ok(t) => t,
        Err(e) => {
            c.schema(format!("the front block is not valid TOML: {}", e.message()));
            return None;
        }
    };
    for k in doc.keys() {
        if !TOP_KEYS.contains(&k.as_str()) {
            c.schema(format!("unknown front-block key `{k}`"));
        }
    }
    match doc.get("format") {
        Some(toml::Value::String(s)) if s == SPEC_FORMAT => {}
        _ => {
            c.schema(format!("format must be {SPEC_FORMAT:?}"));
            return None;
        }
    }
    let id = match doc.get("id") {
        Some(toml::Value::String(s)) if s.strip_prefix("comp.").is_some_and(is_ident) => Some(s.clone()),
        _ => {
            c.schema("id must be \"comp.<name>\" with <name> matching [a-z][a-z0-9-]*");
            None
        }
    };
    if c.text_field(&doc, "title", "front block").is_none() && !doc.contains_key("title") {
        c.schema("missing key `title`");
    }

    let parts = check_parts(&mut c, doc.get("parts"));
    let interactive = parts.values().any(|a| *a);
    let applies = check_states(&mut c, doc.get("states"), interactive);
    check_tokens(&mut c, doc.get("tokens"), &parts, &applies);
    check_density(&mut c, doc.get("density"), &parts);
    let actions = check_keyboard(&mut c, doc.get("keyboard"), interactive);
    if let Some(v) = doc.get("pointer_actions") {
        check_pointer(&mut c, v, &parts, &actions);
    }
    let disabled = applies.get("disabled").copied().unwrap_or(false);
    check_a11y(&mut c, doc.get("a11y"), disabled);
    check_focus(&mut c, doc.get("focus"));
    check_disabled_reason(&mut c, doc.get("disabled_reason"), &parts, disabled);
    id
}

/// `[[parts]]`: name to actionable.
fn check_parts(c: &mut Ctx<'_>, v: Option<&toml::Value>) -> BTreeMap<String, bool> {
    let mut parts = BTreeMap::new();
    let Some(v) = v else {
        c.schema("missing key `parts`");
        return parts;
    };
    let items = c.table_array(v, "parts", &["actionable", "description", "name"], &["name"]);
    for (i, t) in items.into_iter().enumerate() {
        let at = format!("parts[{i}]");
        let actionable = match t.get("actionable") {
            None => false,
            Some(toml::Value::Boolean(b)) => *b,
            Some(_) => {
                c.schema(format!("{at}.actionable must be a boolean"));
                false
            }
        };
        if t.contains_key("description") {
            c.text_field(t, "description", &at);
        }
        let Some(name) = c.ident_field(t, "name", &at) else {
            continue;
        };
        if parts.insert(name.clone(), actionable).is_some() {
            c.schema(format!("part `{name}` is declared twice"));
        }
    }
    if parts.is_empty() {
        c.schema("parts must declare at least one anatomy part");
    }
    parts
}

/// `[states]`: state to applies (only for listed, valid states).
fn check_states(c: &mut Ctx<'_>, v: Option<&toml::Value>, interactive: bool) -> BTreeMap<&'static str, bool> {
    let mut out = BTreeMap::new();
    let table = match v {
        Some(v) => c.table(v, "states", SCHEMA),
        None => None,
    };
    let empty = toml::Table::new();
    let table = table.unwrap_or(&empty);
    for k in table.keys() {
        if !STATES.contains(&k.as_str()) {
            c.schema(format!("states: unknown state `{k}`"));
        }
    }
    for st in STATES {
        let Some(sv) = table.get(st) else {
            c.push(
                STATE_MISSING,
                st,
                "not listed (a state that does not apply is given as applies = false with a reason)",
            );
            continue;
        };
        let at = format!("states.{st}");
        let Some(t) = c.table(sv, &at, SCHEMA) else {
            continue;
        };
        for k in t.keys() {
            if k != "applies" && k != "reason" {
                c.schema(format!("{at}: unknown key `{k}`"));
            }
        }
        let applies = match t.get("applies") {
            Some(toml::Value::Boolean(b)) => *b,
            _ => {
                c.schema(format!("{at}.applies must be a boolean"));
                continue;
            }
        };
        match (applies, t.get("reason")) {
            (true, Some(_)) => c.schema(format!("{at}: a reason is given only when the state does not apply")),
            (false, None) => c.schema(format!("{at}: a state that does not apply needs a reason")),
            (false, Some(_)) => {
                c.text_field(t, "reason", &at);
            }
            (true, None) => {}
        }
        out.insert(st, applies);
    }
    if out.get("default") == Some(&false) {
        c.schema("states.default must apply");
    }
    if interactive && out.get("focus") == Some(&false) {
        c.at_file(FOCUS, "an interactive spec (a part is actionable) must apply the focus state");
    }
    out
}

/// `[tokens.<part>.<state>]`.
fn check_tokens(
    c: &mut Ctx<'_>,
    v: Option<&toml::Value>,
    parts: &BTreeMap<String, bool>,
    applies: &BTreeMap<&'static str, bool>,
) {
    let Some(v) = v else {
        c.schema("missing key `tokens`");
        return;
    };
    let Some(table) = c.table(v, "tokens", SCHEMA) else {
        return;
    };
    for (part, pv) in table {
        let at = format!("tokens.{part}");
        if !parts.contains_key(part) {
            c.schema(format!("{at}: `{part}` is not a declared part"));
        }
        let Some(states) = c.table(pv, &at, SCHEMA) else {
            continue;
        };
        for (st, sv) in states {
            let at = format!("tokens.{part}.{st}");
            match applies.get(st.as_str()) {
                Some(true) => {}
                Some(false) => c.schema(format!("{at}: the state `{st}` does not apply")),
                None if STATES.contains(&st.as_str()) => {}
                None => c.schema(format!("{at}: `{st}` is not a state")),
            }
            let Some(props) = c.table(sv, &at, SCHEMA) else {
                continue;
            };
            if props.is_empty() {
                c.schema(format!("{at} is empty"));
            }
            for (prop, tv) in props {
                if !is_ident(prop) {
                    c.schema(format!("{at}: property `{prop}` must match [a-z][a-z0-9-]*"));
                }
                c.token(tv, &format!("{at}.{prop}"));
            }
        }
    }
    for part in parts.keys() {
        let has_default = matches!(table.get(part), Some(toml::Value::Table(t)) if t.contains_key("default"));
        if !has_default {
            c.schema(format!("part `{part}` has no tokens for the default state"));
        }
    }
}

/// `[density.<part>.<mode>]`.
fn check_density(c: &mut Ctx<'_>, v: Option<&toml::Value>, parts: &BTreeMap<String, bool>) {
    let Some(v) = v else {
        c.at_file(DENSITY, "no density table (compact and comfortable values per part)");
        return;
    };
    let Some(table) = c.table(v, "density", DENSITY) else {
        return;
    };
    if table.is_empty() {
        c.at_file(DENSITY, "density lists no part");
    }
    for (part, pv) in table {
        let at = format!("density.{part}");
        if !parts.contains_key(part) {
            c.at_file(DENSITY, format!("{at}: `{part}` is not a declared part"));
        }
        let Some(modes) = c.table(pv, &at, DENSITY) else {
            continue;
        };
        for k in modes.keys() {
            if !DENSITY_MODES.contains(&k.as_str()) {
                c.at_file(DENSITY, format!("{at}: unknown density mode `{k}`"));
            }
        }
        let mut keysets: Vec<BTreeSet<String>> = Vec::new();
        for mode in ["compact", "comfortable"] {
            let at = format!("{at}.{mode}");
            let Some(mv) = modes.get(mode) else {
                c.at_file(DENSITY, format!("{at} is missing"));
                continue;
            };
            let Some(props) = c.table(mv, &at, DENSITY) else {
                continue;
            };
            if props.is_empty() {
                c.at_file(DENSITY, format!("{at} is empty"));
            }
            for (prop, tv) in props {
                if !is_ident(prop) {
                    c.schema(format!("{at}: property `{prop}` must match [a-z][a-z0-9-]*"));
                }
                c.token(tv, &format!("{at}.{prop}"));
            }
            keysets.push(props.keys().cloned().collect());
        }
        if let [a, b] = keysets.as_slice() {
            if a != b {
                c.at_file(DENSITY, format!("{at}: compact and comfortable give different properties"));
            }
        }
    }
}

/// `[[keyboard]]`: the set of keyboard actions.
fn check_keyboard(c: &mut Ctx<'_>, v: Option<&toml::Value>, interactive: bool) -> BTreeSet<String> {
    let mut actions = BTreeSet::new();
    let Some(v) = v else {
        c.schema("missing key `keyboard` (keyboard = [] for a component with no interaction)");
        return actions;
    };
    let keys = ["action", "context", "key"];
    let items = c.table_array(v, "keyboard", &keys, &keys);
    let mut seen = BTreeSet::new();
    for (i, t) in items.into_iter().enumerate() {
        let at = format!("keyboard[{i}]");
        let key = c.text_field(t, "key", &at);
        let context = c.text_field(t, "context", &at);
        if let Some(a) = c.ident_field(t, "action", &at) {
            actions.insert(a);
        }
        if let (Some(k), Some(x)) = (key, context) {
            if !seen.insert((k.clone(), x)) {
                c.schema(format!("{at}: key `{k}` is listed twice for the same context"));
            }
        }
    }
    if interactive && seen.is_empty() {
        c.schema("an interactive spec (a part is actionable) needs keyboard entries (REQ-UX.COMP-002)");
    }
    actions
}

/// `[[pointer_actions]]` (REQ-UX.COMP.SCHEMA-003).
fn check_pointer(c: &mut Ctx<'_>, v: &toml::Value, parts: &BTreeMap<String, bool>, actions: &BTreeSet<String>) {
    let keys = ["action", "gesture", "part"];
    let items = c.table_array(v, "pointer_actions", &keys, &keys);
    for (i, t) in items.into_iter().enumerate() {
        let at = format!("pointer_actions[{i}]");
        let gesture = c.text_field(t, "gesture", &at).unwrap_or_default();
        let part = c.ident_field(t, "part", &at);
        if let Some(p) = &part {
            match parts.get(p) {
                Some(true) => {}
                Some(false) => c.schema(format!("{at}: part `{p}` is not actionable")),
                None => c.schema(format!("{at}: `{p}` is not a declared part")),
            }
        }
        let Some(action) = c.ident_field(t, "action", &at) else {
            continue;
        };
        if !actions.contains(&action) {
            let p = part.unwrap_or_default();
            c.push(
                POINTER_ONLY,
                &action,
                format!("pointer gesture `{gesture}` on part `{p}` has no keyboard entry with action `{action}`"),
            );
        }
    }
}

/// `[a11y]` (REQ-UX.COMP.SCHEMA-006).
fn check_a11y(c: &mut Ctx<'_>, v: Option<&toml::Value>, disabled: bool) {
    let Some(v) = v else {
        c.schema("missing key `a11y`");
        c.at_file(ROLE, "no a11y.role");
        return;
    };
    let Some(t) = c.table(v, "a11y", SCHEMA) else {
        c.at_file(ROLE, "no a11y.role");
        return;
    };
    for k in t.keys() {
        if !["name_source", "role", "states"].contains(&k.as_str()) {
            c.schema(format!("a11y: unknown key `{k}`"));
        }
    }
    match t.get("role") {
        Some(toml::Value::String(r)) if ROLES.binary_search(&r.as_str()).is_ok() => {}
        Some(toml::Value::String(r)) => {
            let subject = if r.len() <= 64 { r.clone() } else { "a11y.role".to_string() };
            c.push(ROLE, &subject, "not a role of the closed list (WAI-ARIA 1.2 role names)");
        }
        _ => c.at_file(ROLE, "a11y.role must be a role name string"),
    }
    if c.text_field(t, "name_source", "a11y").is_none() && !t.contains_key("name_source") {
        c.schema("a11y: missing key `name_source`");
    }
    let mut exposed = BTreeSet::new();
    match t.get("states") {
        Some(toml::Value::Array(items)) if items.len() <= MAX_ITEMS => {
            for s in items {
                match s {
                    toml::Value::String(s) if ARIA_STATES.binary_search(&s.as_str()).is_ok() => {
                        if !exposed.insert(s.clone()) {
                            c.schema(format!("a11y.states lists `{s}` twice"));
                        }
                    }
                    toml::Value::String(s) => c.schema(format!(
                        "a11y.states: `{s}` is not a WAI-ARIA 1.2 state or property name (without aria-)"
                    )),
                    other => c.schema(format!("a11y.states entries must be strings, not {}", type_name(other))),
                }
            }
        }
        _ => c.schema("a11y.states must be an array of state names (states = [] when none)"),
    }
    if disabled && !exposed.contains("disabled") {
        c.schema("the disabled state applies but a11y.states does not expose `disabled`");
    }
}

/// `[focus]` (REQ-UX.COMP.SCHEMA-004).
fn check_focus(c: &mut Ctx<'_>, v: Option<&toml::Value>) {
    let Some(v) = v else {
        c.at_file(FOCUS, "no focus table (indicator token, surfaces, width and offset)");
        return;
    };
    let Some(t) = c.table(v, "focus", FOCUS) else {
        return;
    };
    let keys = ["against", "offset_px", "token", "width_px"];
    for k in t.keys() {
        if !keys.contains(&k.as_str()) {
            c.schema(format!("focus: unknown key `{k}`"));
        }
    }
    for k in keys {
        if !t.contains_key(k) {
            c.at_file(FOCUS, format!("focus: missing key `{k}`"));
        }
    }
    match t.get("width_px") {
        Some(toml::Value::Integer(w)) if *w < MIN_FOCUS_WIDTH_PX => {
            c.at_file(FOCUS, format!("focus.width_px {w} is thinner than {MIN_FOCUS_WIDTH_PX} px"));
        }
        Some(toml::Value::Integer(w)) if *w > MAX_FOCUS_PX => {
            c.schema(format!("focus.width_px {w} is above {MAX_FOCUS_PX}"));
        }
        Some(toml::Value::Integer(_)) | None => {}
        Some(_) => c.at_file(FOCUS, "focus.width_px must be an integer number of logical pixels"),
    }
    match t.get("offset_px") {
        Some(toml::Value::Integer(o)) if (-MAX_FOCUS_PX..=MAX_FOCUS_PX).contains(o) => {}
        None => {}
        Some(_) => c.schema(format!("focus.offset_px must be an integer from -{MAX_FOCUS_PX} to {MAX_FOCUS_PX}")),
    }
    let token = t.get("token").and_then(|v| c.token(v, "focus.token"));
    let mut surfaces = Vec::new();
    match t.get("against") {
        Some(toml::Value::Array(items)) if !items.is_empty() && items.len() <= 16 => {
            for (i, s) in items.iter().enumerate() {
                if let Some(n) = c.token(s, &format!("focus.against[{i}]")) {
                    surfaces.push(n);
                }
            }
        }
        Some(_) => c.at_file(FOCUS, "focus.against must list 1 to 16 surface tokens"),
        None => {}
    }
    let Some(fg) = token else {
        return;
    };
    for bg in surfaces {
        let subject = format!("{fg}/{bg}");
        match c.pairs {
            Err(reason) => {
                c.push(FOCUS, &subject, format!("the focus pair cannot be checked as non-text: {reason}"));
            }
            Ok(pairs) => match pairs.get(&(fg.clone(), bg.clone())).map(String::as_str) {
                Some("non-text") => {}
                Some(tag) => c.push(FOCUS, &subject, format!("declared {tag} in {PAIRS_FILE}, not non-text")),
                None => c.push(FOCUS, &subject, format!("not declared non-text in {PAIRS_FILE}")),
            },
        }
    }
}

/// `[disabled_reason]` (REQ-UX.COMP.SCHEMA-005).
fn check_disabled_reason(c: &mut Ctx<'_>, v: Option<&toml::Value>, parts: &BTreeMap<String, bool>, disabled: bool) {
    let Some(v) = v else {
        if disabled {
            c.at_file(
                DISABLED_REASON,
                "the disabled state applies but no disabled_reason shows the reason as visible or focus-reachable text",
            );
        }
        return;
    };
    let Some(t) = c.table(v, "disabled_reason", DISABLED_REASON) else {
        return;
    };
    for k in t.keys() {
        if !["note", "part", "shown_as"].contains(&k.as_str()) {
            c.schema(format!("disabled_reason: unknown key `{k}`"));
        }
    }
    match t.get("shown_as") {
        Some(toml::Value::String(m)) if REASON_MODES.contains(&m.as_str()) => {}
        _ => c.at_file(
            DISABLED_REASON,
            "disabled_reason.shown_as must be \"visible\" or \"focus-reachable\" (never pointer hover alone)",
        ),
    }
    match t.get("part") {
        Some(toml::Value::String(p)) if parts.contains_key(p) => {}
        _ => c.at_file(DISABLED_REASON, "disabled_reason.part must name the declared part that carries the text"),
    }
    if t.contains_key("note") {
        c.text_field(t, "note", "disabled_reason");
    }
}

impl Report {
    pub fn exit_code(&self) -> u8 {
        u8::from(!self.findings.is_empty())
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
        let mut m = BTreeMap::new();
        m.insert("findings".to_string(), findings_value(&self.findings));
        m.insert("format".to_string(), CValue::Str(REPORT_FORMAT.to_string()));
        m.insert("ok".to_string(), CValue::Bool(self.findings.is_empty()));
        m.insert("specs".to_string(), CValue::Arr(specs));
        CValue::Obj(m)
    }

    /// RFC 8785 canonical JSON.
    pub fn to_json(&self) -> Result<Vec<u8>, fid_canon::CanonError> {
        fid_canon::to_canonical_bytes(&self.to_cvalue())
    }

    pub fn to_text(&self) -> String {
        let mut s = format!("fid-design comp-check: {} spec(s)\n", self.specs.len());
        for x in &self.specs {
            s.push_str(&format!("  {}: {}\n", x.file, x.id.as_deref().unwrap_or("(no valid id)")));
        }
        findings_text(&mut s, &self.findings);
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closed_lists_are_sorted_and_unique() {
        for w in ROLES.windows(2) {
            assert!(w[0] < w[1], "{} !< {}", w[0], w[1]);
        }
        for w in ARIA_STATES.windows(2) {
            assert!(w[0] < w[1], "{} !< {}", w[0], w[1]);
        }
        assert!(TOP_KEYS.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn idents() {
        assert!(is_ident("read-only"));
        assert!(is_ident("a1"));
        assert!(!is_ident("1a"));
        assert!(!is_ident("A"));
        assert!(!is_ident(""));
        assert!(!is_ident("a_b"));
    }

    #[test]
    fn front_block_split() {
        let (b, p) = split_front("```toml\na = 1\n```\n## Anatomy\n").unwrap();
        assert_eq!(b, "a = 1\n");
        assert_eq!(p, vec!["## Anatomy"]);
        assert!(split_front("# Title\n```toml\n```\n").is_err());
        assert!(split_front("```toml\na = 1\n").is_err());
        let (b, _) = split_front("```toml\r\na = 1\r\n```\r\n").unwrap();
        assert_eq!(b, "a = 1\n");
    }
}
