//! `fid-design tokens-check --root <design dir> [--json]` (spec node `UX.TOKENS.COLOR`): the
//! structure of the colour tokens, their per-theme resolution, the component tier, the declared
//! contrast pairs and the brand tier. Contrast values are not gated here; that is
//! `UX.TOKENS.CHECK`. The WCAG formula itself is in [`wcag`].
//!
//! Inputs below `<root>/tokens/`:
//! - `**/*.tokens.json`: DTCG token files, read with `fid_design_pkg::tokens::collect`. A
//!   colour token is one whose (declared or inherited) `$type` is `color`. Colour tokens are named
//!   `<tier>.<group>.<role>[.<variant>]`: `prim.color.<hue>.<step>` (literal colours),
//!   `sem.<group>.<role>[.<variant>]` with group in [`SEM_GROUPS`], and
//!   `comp.<component>.<role>[.<variant>]`. Colour tokens of the `data` and `pal` tiers belong to
//!   `UX.TOKENS.DATAPAL` and are only looked at by the brand rule.
//! - `themes/{light,dark,hc,floor}.json` (`fid-design-theme@1`):
//!   `{ "format", "theme", "bindings": { "<sem name>": "{<target>}" } }`. A theme binds every
//!   semantic colour token by alias; a semantic token's own `$value` in its token file must equal
//!   its `light` binding (it is the default a plain DTCG reader sees).
//! - `pairs.json` (`fid-design-pairs@1`): `{ "format", "pairs": [{ "fg", "bg", "tag" }] }` with
//!   tag `text`, `large-text` or `non-text`; fg and bg are semantic colour tokens.
//! - `brand-tier.json` (`fid-design-brand-tier@1`): `{ "format", "tokens": ["<name>"] }`.
//!
//! Resolution in a theme: a semantic token follows its theme binding, any other token follows
//! its `{alias}`, and the chain must end at a `prim.*` token holding a literal colour, within
//! [`MAX_ALIAS_DEPTH`] hops and without a cycle. A finding about resolution names the theme and
//! the first token where the chain breaks, so a missing binding is reported once even when
//! component tokens alias it.
//!
//! Rules (findings, exit 1):
//! - `TOK-UNRESOLVED <theme> <name>`: no binding, an alias to a name that is not a token, a
//!   literal outside the prim tier, or more than [`MAX_ALIAS_DEPTH`] hops.
//! - `TOK-ALIAS-CYCLE <theme> <name>`: the chain returns to a name (the smallest name in the cycle).
//! - `TOK-COMP-SKIPS-SEM <name>`: a component colour token that does not alias a `sem.*` token.
//! - `TOK-NAME-SYNTAX <name>`: a colour token name outside the tier grammar.
//! - `TOK-BRAND-SIZE`: more than [`MAX_BRAND_TIER`] names, or a name outside `sem.brand.*` and
//!   [`BRAND_EXTRA`].
//! - `TOK-BRAND-STATUS <brand name>`: in some theme, a brand-tier token's chain reaches a
//!   `sem.status.*`, `sem.data.*`, `data.*` or `pal.*` token, or such a token's chain reaches a
//!   brand-tier token.
//! - `TOK-PAIR-UNKNOWN <name>`: a pair names something that is not a semantic colour token.
//! - `TOK-CLAIMED-AS-MEASURED` (added by this leaf for the maintainer rule "claimed values never
//!   look measured", UNC.UEC.CLASS): `sem.text.value.claimed` and `sem.text.value.measured` must
//!   exist; no chain of a `*.claimed` token may pass through a `*.measured` or brand-tier token
//!   (and the other way round), so neither a skin nor an alias can merge them; a component
//!   `*.claimed`/`*.measured` token must alias a `sem.*.value.claimed`/`.measured` role; and in
//!   every theme the `sem.text.*` and `sem.border.*` claimed roles must resolve to a primitive
//!   hue ramp (`prim.color.<ramp>.*`) that no `sem.*.value.measured` role uses. Colour stays a
//!   second cue only (principle P-NOCOLOR): the word "claimed" is required by the components.
//! - `TOK-SCHEMA <file or name>`: an unreadable shape: invalid JSON, a missing file, wrong format
//!   ID, a binding of a non-semantic name, a semantic default that differs from the light
//!   binding, a primitive that is not a literal, a bad pair tag, duplicates.
//!
//! Exit codes: 0 no findings, 1 findings, 2 unreadable root or limits exceeded. Determinism D0:
//! sorted walk, `BTreeMap` only, no floating point, no clock.

pub mod wcag;

use crate::finding::{findings_text, findings_value, read_text, Finding};
use fid_canon::CValue;
use fid_design_pkg::tokens::{collect, Token};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Report format ID.
pub const REPORT_FORMAT: &str = "fid-design-tokens-check@1";
/// Theme file format ID.
pub const THEME_FORMAT: &str = "fid-design-theme@1";
/// Pairs file format ID.
pub const PAIRS_FORMAT: &str = "fid-design-pairs@1";
/// Brand-tier file format ID.
pub const BRAND_FORMAT: &str = "fid-design-brand-tier@1";
/// The four themes, in the order the spec lists them.
pub const THEMES: [&str; 4] = ["light", "dark", "hc", "floor"];
/// Groups of the semantic tier.
pub const SEM_GROUPS: [&str; 8] = ["agent", "border", "brand", "data", "focus", "status", "surface", "text"];
/// Pair tags.
pub const PAIR_TAGS: [&str; 3] = ["large-text", "non-text", "text"];
/// Most names in the brand tier [unverified bound, REQ-UX.TOKENS.COLOR-005].
pub const MAX_BRAND_TIER: usize = 12;
/// The one brand-tier name outside `sem.brand.*` (owned by `UX.TOKENS.TYPE`).
pub const BRAND_EXTRA: &str = "type.family.display";
/// Most alias hops followed from one name.
pub const MAX_ALIAS_DEPTH: usize = fid_design_pkg::MAX_ALIAS_DEPTH;
/// The required claimed and measured text roles.
pub const CLAIMED_TEXT: &str = "sem.text.value.claimed";
pub const MEASURED_TEXT: &str = "sem.text.value.measured";

const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 64 * 1024 * 1024;
const MAX_FILES: usize = 20_000;
const MAX_DIR_DEPTH: usize = 16;

pub const UNRESOLVED: &str = "TOK-UNRESOLVED";
pub const ALIAS_CYCLE: &str = "TOK-ALIAS-CYCLE";
pub const COMP_SKIPS_SEM: &str = "TOK-COMP-SKIPS-SEM";
pub const NAME_SYNTAX: &str = "TOK-NAME-SYNTAX";
pub const BRAND_STATUS: &str = "TOK-BRAND-STATUS";
pub const BRAND_SIZE: &str = "TOK-BRAND-SIZE";
pub const PAIR_UNKNOWN: &str = "TOK-PAIR-UNKNOWN";
pub const CLAIMED_AS_MEASURED: &str = "TOK-CLAIMED-AS-MEASURED";
pub const SCHEMA: &str = "TOK-SCHEMA";

/// One declared contrast pair.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Pair {
    pub fg: String,
    pub bg: String,
    pub tag: String,
}

/// A resolved colour: the primitive that holds it and its literal value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Value {
    pub prim: String,
    pub value: String,
}

/// Per-theme resolution counts and values.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ThemeResult {
    pub semantic_resolved: usize,
    pub semantic_unresolved: usize,
    pub component_resolved: usize,
    pub component_unresolved: usize,
    /// Every resolved semantic and component colour token.
    pub values: BTreeMap<String, Value>,
}

/// Result of `tokens-check`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    pub primitive: usize,
    pub semantic: usize,
    pub component: usize,
    pub brand_tier: Vec<String>,
    pub pairs: Vec<Pair>,
    /// By theme name.
    pub themes: BTreeMap<String, ThemeResult>,
    /// Sorted, de-duplicated.
    pub findings: Vec<Finding>,
}

/// The contrast of one pair in one theme (see [`Report::pair_contrasts`]).
#[derive(Debug, Clone, PartialEq)]
pub struct PairContrast {
    pub theme: String,
    pub pair: Pair,
    /// `None` when either side did not resolve or has alpha.
    pub ratio: Option<f64>,
    /// `None` for an unknown tag.
    pub required: Option<f64>,
}

/// Runs the check on `<root>/tokens/`. `Err` (exit 2): the root or a file cannot be read, or
/// a limit is exceeded.
pub fn check(root: &Path) -> Result<Report, String> {
    let mut f = Vec::new();
    let input = Input::load(root, &mut f)?;
    Ok(input.check(f))
}

// ------------------------------------------------------------------ input

/// `(code, broken name, detail)`.
type Broken = (&'static str, String, String);
/// The names visited (start first) and the value or the break.
type Resolution = (Vec<String>, Result<Value, Broken>);

struct Input {
    tokens: BTreeMap<String, Token>,
    prim: BTreeSet<String>,
    sem: BTreeSet<String>,
    comp: BTreeSet<String>,
    /// `None` when the theme file is missing or unusable.
    themes: BTreeMap<&'static str, Option<BTreeMap<String, String>>>,
    pairs: Vec<Pair>,
    brand: Vec<String>,
}

fn tier(name: &str) -> &str {
    name.split('.').next().unwrap_or("")
}

fn last(name: &str) -> &str {
    name.rsplit('.').next().unwrap_or("")
}

/// Names the brand tier may neither alias nor be aliased by (REQ-UX.TOKENS.COLOR-005).
fn forbidden(n: &str) -> bool {
    n.starts_with("sem.status.") || n.starts_with("sem.data.") || n.starts_with("data.") || n.starts_with("pal.")
}

fn is_colour(t: &Token) -> bool {
    t.ty.as_deref() == Some("color")
}

/// Why a colour token name breaks the tier grammar, if it does.
fn name_defect(name: &str) -> Option<&'static str> {
    if !fid_design_pkg::is_valid_name(name) {
        return Some("name does not match ^[a-z][a-z0-9]*(\\.[a-z0-9-]+)*$");
    }
    let segs: Vec<&str> = name.split('.').collect();
    let n = segs.len();
    match segs.first().copied() {
        Some("prim") if n != 4 || segs.get(1) != Some(&"color") => {
            Some("primitive colour tokens are named prim.color.<hue>.<step>")
        }
        Some("sem") if !(3..=4).contains(&n) || !segs.get(1).is_some_and(|g| SEM_GROUPS.contains(g)) => Some(
            "semantic colour tokens are named sem.<group>.<role>[.<variant>] with group in agent, border, brand, data, focus, status, surface, text",
        ),
        Some("comp") if !(3..=4).contains(&n) => {
            Some("component colour tokens are named comp.<component>.<role>[.<variant>]")
        }
        Some("prim" | "sem" | "comp" | "data" | "pal") => None,
        _ => Some("colour tokens are named <tier>.<group>.<role>[.<variant>] with tier prim, sem or comp"),
    }
}

fn path_of(root: &Path, rel: &str) -> PathBuf {
    rel.split('/').fold(root.to_path_buf(), |p, s| p.join(s))
}

fn io(path: &Path, e: std::io::Error) -> String {
    format!("cannot read {}: {e}", path.display())
}

/// Sorted list of `tokens/**/*.tokens.json` (package-relative path, file path).
fn token_files(root: &Path, f: &mut Vec<Finding>) -> Result<Vec<(String, PathBuf)>, String> {
    let dir = root.join("tokens");
    let md = std::fs::symlink_metadata(&dir).map_err(|e| io(&dir, e))?;
    if !md.is_dir() {
        return Err(format!("{} is not a directory", dir.display()));
    }
    let mut out = Vec::new();
    let mut count = 0usize;
    let mut stack: Vec<(PathBuf, String, usize)> = vec![(dir, "tokens".to_string(), 0)];
    while let Some((d, rel, depth)) = stack.pop() {
        let mut entries = Vec::new();
        for e in std::fs::read_dir(&d).map_err(|e| io(&d, e))? {
            let e = e.map_err(|e| io(&d, e))?;
            entries.push((e.file_name(), e.path()));
        }
        entries.sort();
        for (name, path) in entries {
            count += 1;
            if count > MAX_FILES {
                return Err(format!("more than {MAX_FILES} files under {}", root.join("tokens").display()));
            }
            let Some(name) = name.to_str() else {
                f.push(Finding::new(SCHEMA, &rel, "a file name is not UTF-8; not read"));
                continue;
            };
            let child = format!("{rel}/{name}");
            let m = std::fs::symlink_metadata(&path).map_err(|e| io(&path, e))?;
            if m.file_type().is_symlink() {
                f.push(Finding::new(SCHEMA, &child, "symbolic link; not followed"));
            } else if m.is_dir() {
                if depth + 1 > MAX_DIR_DEPTH {
                    return Err(format!("{child}: deeper than {MAX_DIR_DEPTH} directories"));
                }
                stack.push((path, child, depth + 1));
            } else if m.is_file() && name.ends_with(".tokens.json") {
                out.push((child, path));
            }
        }
    }
    out.sort();
    Ok(out)
}

/// Reads and parses one JSON file. A missing, non-regular or invalid file is a finding and
/// `Ok(None)`; an I/O failure or an exceeded limit is `Err`.
fn read_json(root: &Path, rel: &str, budget: &mut u64, f: &mut Vec<Finding>) -> Result<Option<CValue>, String> {
    let p = path_of(root, rel);
    match std::fs::symlink_metadata(&p) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            f.push(Finding::new(SCHEMA, rel, "missing"));
            return Ok(None);
        }
        Err(e) => return Err(io(&p, e)),
        Ok(m) if !m.is_file() => {
            f.push(Finding::new(SCHEMA, rel, "not a regular file (symbolic links are not followed)"));
            return Ok(None);
        }
        Ok(_) => {}
    }
    let text = read_text(&p, MAX_FILE_BYTES)?;
    *budget = budget.saturating_add(text.len() as u64);
    if *budget > MAX_TOTAL_BYTES {
        return Err(format!("token files exceed {MAX_TOTAL_BYTES} bytes in total (at {rel})"));
    }
    match fid_canon::parse_with_limits(text.as_bytes(), MAX_FILE_BYTES as usize, fid_design_pkg::MAX_JSON_DEPTH) {
        Ok(v) => Ok(Some(v)),
        Err(e) => {
            f.push(Finding::new(SCHEMA, rel, format!("invalid JSON: {e}")));
            Ok(None)
        }
    }
}

/// The object's members when its keys are exactly `keys` (sorted) and `format` is `format`.
fn header<'a>(
    v: &'a CValue,
    rel: &str,
    keys: &[&str],
    format: &str,
    f: &mut Vec<Finding>,
) -> Option<&'a BTreeMap<String, CValue>> {
    let shape = format!("must be an object with exactly the keys {} and format {format:?}", keys.join(", "));
    let CValue::Obj(m) = v else {
        f.push(Finding::new(SCHEMA, rel, shape));
        return None;
    };
    let got: Vec<&str> = m.keys().map(String::as_str).collect();
    if got != keys || m.get("format") != Some(&CValue::Str(format.to_string())) {
        f.push(Finding::new(SCHEMA, rel, shape));
        return None;
    }
    Some(m)
}

fn alias_target(v: &CValue) -> Option<&str> {
    match v {
        CValue::Str(s) => fid_design_pkg::tokens::parse_alias(s).filter(|t| fid_design_pkg::is_valid_name(t)),
        _ => None,
    }
}

impl Input {
    fn load(root: &Path, f: &mut Vec<Finding>) -> Result<Input, String> {
        let mut budget = 0u64;
        let mut all = Vec::new();
        let mut pf = Vec::new();
        for (rel, _) in token_files(root, f)? {
            if let Some(v) = read_json(root, &rel, &mut budget, f)? {
                collect(&rel, &v, &mut all, &mut pf);
            }
        }
        for x in pf {
            let code = if x.code == fid_design_pkg::Code::NameSyntax { NAME_SYNTAX } else { SCHEMA };
            f.push(Finding::new(code, x.subject, format!("{}: {}", x.code.as_str(), x.detail)));
        }
        let mut tokens: BTreeMap<String, Token> = BTreeMap::new();
        for t in all {
            if let Some(first) = tokens.get(&t.name) {
                f.push(Finding::new(SCHEMA, &t.name, format!("defined in {} and {}", first.file, t.file)));
            } else {
                tokens.insert(t.name.clone(), t);
            }
        }
        let (mut prim, mut sem, mut comp) = (BTreeSet::new(), BTreeSet::new(), BTreeSet::new());
        for t in tokens.values().filter(|t| is_colour(t)) {
            if let Some(d) = name_defect(&t.name) {
                f.push(Finding::new(NAME_SYNTAX, &t.name, d));
            }
            match tier(&t.name) {
                "prim" => prim.insert(t.name.clone()),
                "sem" => sem.insert(t.name.clone()),
                "comp" => comp.insert(t.name.clone()),
                _ => false,
            };
        }

        let mut themes = BTreeMap::new();
        for theme in THEMES {
            let rel = format!("tokens/themes/{theme}.json");
            let parsed = match read_json(root, &rel, &mut budget, f)? {
                Some(v) => Self::parse_theme(&v, &rel, theme, f),
                None => None,
            };
            themes.insert(theme, parsed);
        }

        let mut pairs = Vec::new();
        if let Some(v) = read_json(root, "tokens/pairs.json", &mut budget, f)? {
            pairs = Self::parse_pairs(&v, f);
        }
        let mut brand = Vec::new();
        if let Some(v) = read_json(root, "tokens/brand-tier.json", &mut budget, f)? {
            brand = Self::parse_brand(&v, f);
        }
        Ok(Input { tokens, prim, sem, comp, themes, pairs, brand })
    }

    fn parse_theme(v: &CValue, rel: &str, theme: &str, f: &mut Vec<Finding>) -> Option<BTreeMap<String, String>> {
        let m = header(v, rel, &["bindings", "format", "theme"], THEME_FORMAT, f)?;
        if m.get("theme") != Some(&CValue::Str(theme.to_string())) {
            f.push(Finding::new(SCHEMA, rel, format!("theme must be {theme:?}")));
            return None;
        }
        let Some(CValue::Obj(b)) = m.get("bindings") else {
            f.push(Finding::new(SCHEMA, rel, "bindings must be an object of \"<sem name>\": \"{<target>}\""));
            return None;
        };
        let mut out = BTreeMap::new();
        for (k, v) in b {
            match alias_target(v) {
                Some(t) => {
                    out.insert(k.clone(), t.to_string());
                }
                None => f.push(Finding::new(
                    SCHEMA,
                    format!("{theme} {k}"),
                    "a binding is a whole alias \"{<name>}\" (themes bind roles to primitives, never to literals)",
                )),
            }
        }
        Some(out)
    }

    fn parse_pairs(v: &CValue, f: &mut Vec<Finding>) -> Vec<Pair> {
        let rel = "tokens/pairs.json";
        let mut out = Vec::new();
        let Some(m) = header(v, rel, &["format", "pairs"], PAIRS_FORMAT, f) else {
            return out;
        };
        let Some(CValue::Arr(items)) = m.get("pairs") else {
            f.push(Finding::new(SCHEMA, rel, "pairs must be an array"));
            return out;
        };
        let mut seen = BTreeSet::new();
        for (i, item) in items.iter().enumerate() {
            let s = |k: &str| match item {
                CValue::Obj(o) if o.len() == 3 => match o.get(k) {
                    Some(CValue::Str(x)) => Some(x.clone()),
                    _ => None,
                },
                _ => None,
            };
            let (Some(fg), Some(bg), Some(tag)) = (s("fg"), s("bg"), s("tag")) else {
                f.push(Finding::new(SCHEMA, rel, format!("pair {i}: needs exactly the string keys bg, fg, tag")));
                continue;
            };
            if !PAIR_TAGS.contains(&tag.as_str()) {
                f.push(Finding::new(SCHEMA, rel, format!("pair {i}: tag {tag:?} is not text, large-text or non-text")));
            }
            if fg == bg {
                f.push(Finding::new(SCHEMA, rel, format!("pair {i}: foreground and background are both {fg}")));
            }
            if !seen.insert((fg.clone(), bg.clone())) {
                f.push(Finding::new(SCHEMA, format!("{fg}/{bg}"), "pair declared twice"));
                continue;
            }
            out.push(Pair { fg, bg, tag });
        }
        out
    }

    fn parse_brand(v: &CValue, f: &mut Vec<Finding>) -> Vec<String> {
        let rel = "tokens/brand-tier.json";
        let mut out = Vec::new();
        let Some(m) = header(v, rel, &["format", "tokens"], BRAND_FORMAT, f) else {
            return out;
        };
        let Some(CValue::Arr(items)) = m.get("tokens") else {
            f.push(Finding::new(SCHEMA, rel, "tokens must be an array of names"));
            return out;
        };
        for (i, item) in items.iter().enumerate() {
            match item {
                CValue::Str(n) if out.contains(n) => f.push(Finding::new(SCHEMA, n, "listed twice in the brand tier")),
                CValue::Str(n) => out.push(n.clone()),
                _ => f.push(Finding::new(SCHEMA, rel, format!("entry {i} is not a string"))),
            }
        }
        out
    }

    // -------------------------------------------------------------- resolution

    /// Follows `start` in `theme`. Returns the names visited (start first) and either the
    /// resolved value or `(code, broken name, detail)`.
    fn resolve(&self, theme: &str, start: &str) -> Resolution {
        let binds = self.themes.get(theme).and_then(Option::as_ref);
        let mut names: Vec<String> = Vec::new();
        let mut cur = start.to_string();
        loop {
            if let Some(pos) = names.iter().position(|n| n == &cur) {
                let at = names[pos..].iter().min().cloned().unwrap_or_else(|| cur.clone());
                let cycle = names[pos..].join(" -> ");
                return (names, Err((ALIAS_CYCLE, at, format!("alias cycle: {cycle} -> {cur}"))));
            }
            if names.len() > MAX_ALIAS_DEPTH {
                let d = format!("more than {MAX_ALIAS_DEPTH} alias hops");
                return (names, Err((UNRESOLVED, start.to_string(), d)));
            }
            names.push(cur.clone());
            let next = if self.sem.contains(&cur) {
                match binds.and_then(|b| b.get(&cur)) {
                    Some(t) => t.clone(),
                    None => {
                        let d = format!("no binding in theme {theme}");
                        return (names, Err((UNRESOLVED, cur, d)));
                    }
                }
            } else if let Some(tok) = self.tokens.get(&cur) {
                match tok.alias() {
                    Some(a) => a.to_string(),
                    None => {
                        let lit = match &tok.value {
                            CValue::Str(s) if self.prim.contains(&cur) => Some(s.clone()),
                            _ => None,
                        };
                        return match lit {
                            Some(value) => (names, Ok(Value { prim: cur, value })),
                            None => {
                                let d = "literal value outside the prim tier; colour tokens resolve to a prim.* colour"
                                    .to_string();
                                (names, Err((UNRESOLVED, cur, d)))
                            }
                        };
                    }
                }
            } else {
                let at = if names.len() >= 2 { names[names.len() - 2].clone() } else { cur.clone() };
                let d = format!("{cur} is not a token");
                return (names, Err((UNRESOLVED, at, d)));
            };
            cur = next;
        }
    }

    // -------------------------------------------------------------- rules

    fn check(&self, mut f: Vec<Finding>) -> Report {
        let brand: BTreeSet<&str> = self.brand.iter().map(String::as_str).collect();

        // Primitives hold literals; components alias the semantic tier.
        for p in &self.prim {
            if self.tokens.get(p).and_then(Token::alias).is_some() {
                f.push(Finding::new(SCHEMA, p, "a primitive colour token holds a literal colour, not an alias"));
            }
        }
        let mut skips = BTreeSet::new();
        for c in &self.comp {
            match self.tokens.get(c).and_then(Token::alias) {
                Some(a) if a.starts_with("sem.") => {}
                Some(a) => {
                    f.push(Finding::new(
                        COMP_SKIPS_SEM,
                        c,
                        format!("aliases {a}; a component token aliases a sem.* token"),
                    ));
                }
                None => {
                    skips.insert(c.clone());
                    f.push(Finding::new(
                        COMP_SKIPS_SEM,
                        c,
                        "holds a literal colour; a component token aliases a sem.* token",
                    ));
                }
            }
        }

        // Theme bindings name semantic tokens; semantic defaults equal the light binding.
        for (theme, binds) in &self.themes {
            for k in binds.iter().flat_map(BTreeMap::keys) {
                if !self.sem.contains(k) {
                    f.push(Finding::new(
                        SCHEMA,
                        format!("{theme} {k}"),
                        "binds a name that is not a semantic colour token (themes bind sem.* tokens only)",
                    ));
                }
            }
        }
        let light = self.themes.get("light").and_then(Option::as_ref);
        for s in &self.sem {
            let default = self.tokens.get(s).and_then(Token::alias);
            match (default, light.and_then(|l| l.get(s))) {
                (None, _) => f.push(Finding::new(
                    SCHEMA,
                    s,
                    "a semantic token's $value is an alias (its light binding), never a literal",
                )),
                (Some(d), Some(l)) if d != l.as_str() => {
                    f.push(Finding::new(SCHEMA, s, format!("$value {{{d}}} differs from the light binding {{{l}}}")))
                }
                _ => {}
            }
        }

        // Pairs name semantic colour tokens.
        for p in &self.pairs {
            for n in [&p.fg, &p.bg] {
                if !self.sem.contains(n) {
                    f.push(Finding::new(
                        PAIR_UNKNOWN,
                        n,
                        format!("pair {}/{}: not a semantic colour token", p.fg, p.bg),
                    ));
                }
            }
        }

        // Brand tier bounds.
        if self.brand.len() > MAX_BRAND_TIER {
            f.push(Finding::new(
                BRAND_SIZE,
                "tokens/brand-tier.json",
                format!("lists {} tokens; at most {MAX_BRAND_TIER}", self.brand.len()),
            ));
        }
        for b in &self.brand {
            if !(b.starts_with("sem.brand.") || b == BRAND_EXTRA) {
                f.push(Finding::new(BRAND_SIZE, b, format!("outside sem.brand.* and {BRAND_EXTRA}")));
            } else if b != BRAND_EXTRA && !self.sem.contains(b) {
                f.push(Finding::new(SCHEMA, b, "brand-tier entry is not a semantic colour token"));
            }
        }

        // Claimed and measured roles exist.
        for n in [CLAIMED_TEXT, MEASURED_TEXT] {
            if !self.sem.contains(n) {
                f.push(Finding::new(
                    CLAIMED_AS_MEASURED,
                    n,
                    "missing: claimed and measured values need their own semantic roles",
                ));
            }
        }
        let claimed: BTreeSet<&str> =
            self.sem.iter().chain(&self.comp).map(String::as_str).filter(|n| last(n) == "claimed").collect();
        let measured: BTreeSet<&str> =
            self.sem.iter().chain(&self.comp).map(String::as_str).filter(|n| last(n) == "measured").collect();
        for c in self.comp.iter().map(String::as_str) {
            let want = match last(c) {
                "claimed" => ".value.claimed",
                "measured" => ".value.measured",
                _ => continue,
            };
            let ok =
                self.tokens.get(c).and_then(Token::alias).is_some_and(|a| a.starts_with("sem.") && a.ends_with(want));
            if !ok {
                f.push(Finding::new(CLAIMED_AS_MEASURED, c, format!("must alias a sem.*{want} role")));
            }
        }

        // Per theme: resolution, brand isolation, claimed/measured isolation.
        let mut starts: BTreeSet<&str> = self.sem.iter().chain(&self.comp).map(String::as_str).collect();
        starts.extend(brand.iter().copied());
        starts.extend(self.tokens.keys().map(String::as_str).filter(|n| forbidden(n)));
        let mut brand_hits: BTreeMap<(String, String, bool), BTreeSet<&str>> = BTreeMap::new();
        let mut themes = BTreeMap::new();
        for theme in THEMES {
            let usable = self.themes.get(theme).is_some_and(Option::is_some);
            let mut r = ThemeResult::default();
            let mut chains: BTreeMap<&str, Vec<String>> = BTreeMap::new();
            for s in &starts {
                let (names, end) = self.resolve(theme, s);
                let is_sem = self.sem.contains(*s);
                let is_comp = self.comp.contains(*s);
                match end {
                    Ok(v) => {
                        if is_sem || is_comp {
                            r.values.insert((*s).to_string(), v);
                        }
                        if is_sem {
                            r.semantic_resolved += 1;
                        } else if is_comp {
                            r.component_resolved += 1;
                        }
                    }
                    Err((code, at, detail)) => {
                        if is_sem {
                            r.semantic_unresolved += 1;
                        } else if is_comp {
                            r.component_unresolved += 1;
                        }
                        if (is_sem || is_comp) && usable && !skips.contains(&at) {
                            f.push(Finding::new(code, format!("{theme} {at}"), detail));
                        }
                    }
                }
                chains.insert(*s, names);
            }
            for (s, names) in &chains {
                let rest = names.get(1..).unwrap_or(&[]);
                if brand.contains(*s) {
                    for n in rest.iter().filter(|n| forbidden(n.as_str())) {
                        brand_hits.entry(((*s).to_string(), n.clone(), true)).or_default().insert(theme);
                    }
                }
                if forbidden(s) {
                    for n in rest.iter().filter(|n| brand.contains(n.as_str())) {
                        brand_hits.entry((n.clone(), (*s).to_string(), false)).or_default().insert(theme);
                    }
                }
                let other = if claimed.contains(*s) {
                    &measured
                } else if measured.contains(*s) {
                    &claimed
                } else {
                    continue;
                };
                for n in rest.iter().filter(|n| other.contains(n.as_str()) || brand.contains(n.as_str())) {
                    f.push(Finding::new(
                        CLAIMED_AS_MEASURED,
                        format!("{theme} {s}"),
                        format!(
                            "resolves through {n}; claimed and measured roles never share an alias or the brand tier"
                        ),
                    ));
                }
            }
            let ramp = |n: &str| r.values.get(n).and_then(|v| v.prim.split('.').nth(2).map(str::to_string));
            let measured_ramps: BTreeSet<String> =
                measured.iter().filter(|n| n.starts_with("sem.")).filter_map(|&n| ramp(n)).collect();
            for &c in claimed.iter().filter(|n| n.starts_with("sem.text.") || n.starts_with("sem.border.")) {
                if let Some(h) = ramp(c) {
                    if measured_ramps.contains(&h) {
                        f.push(Finding::new(
                            CLAIMED_AS_MEASURED,
                            format!("{theme} {c}"),
                            format!("uses the {h} ramp, which a measured-value role also uses"),
                        ));
                    }
                }
            }
            themes.insert(theme.to_string(), r);
        }
        for ((b, n, outward), ts) in brand_hits {
            let ts: Vec<&str> = THEMES.iter().copied().filter(|t| ts.contains(t)).collect();
            let what = if outward { format!("resolves through {n}") } else { format!("is reached from {n}") };
            f.push(Finding::new(
                BRAND_STATUS,
                &b,
                format!("{what} (themes: {}); brand tokens never alias status or data-palette tokens", ts.join(", ")),
            ));
        }

        f.sort();
        f.dedup();
        Report {
            primitive: self.prim.len(),
            semantic: self.sem.len(),
            component: self.comp.len(),
            brand_tier: self.brand.clone(),
            pairs: self.pairs.clone(),
            themes,
            findings: f,
        }
    }
}

// ------------------------------------------------------------------ report

fn num(n: usize) -> CValue {
    CValue::Num(n as f64)
}

fn obj(items: Vec<(&str, CValue)>) -> CValue {
    CValue::Obj(items.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

impl Report {
    pub fn exit_code(&self) -> u8 {
        u8::from(!self.findings.is_empty())
    }

    /// The WCAG contrast of every declared pair in every theme (informational; gating is
    /// `UX.TOKENS.CHECK`). Sorted by theme (spec order), then pair.
    pub fn pair_contrasts(&self) -> Vec<PairContrast> {
        let mut out = Vec::new();
        for theme in THEMES {
            let values = self.themes.get(theme).map(|t| &t.values);
            let rgb = |n: &str| values.and_then(|v| v.get(n)).and_then(|v| wcag::parse_hex(&v.value));
            for p in &self.pairs {
                let ratio = match (rgb(p.fg.as_str()), rgb(p.bg.as_str())) {
                    (Some(a), Some(b)) => Some(wcag::contrast_ratio(a, b)),
                    _ => None,
                };
                out.push(PairContrast {
                    theme: theme.to_string(),
                    pair: p.clone(),
                    ratio,
                    required: wcag::required_ratio(&p.tag, theme),
                });
            }
        }
        out
    }

    pub fn to_cvalue(&self) -> CValue {
        let mut themes = BTreeMap::new();
        for (t, r) in &self.themes {
            let values = r
                .values
                .iter()
                .map(|(n, v)| {
                    (
                        n.clone(),
                        obj(vec![("prim", CValue::Str(v.prim.clone())), ("value", CValue::Str(v.value.clone()))]),
                    )
                })
                .collect();
            themes.insert(
                t.clone(),
                obj(vec![
                    (
                        "semantic",
                        obj(vec![("resolved", num(r.semantic_resolved)), ("unresolved", num(r.semantic_unresolved))]),
                    ),
                    (
                        "component",
                        obj(vec![("resolved", num(r.component_resolved)), ("unresolved", num(r.component_unresolved))]),
                    ),
                    ("values", CValue::Obj(values)),
                ]),
            );
        }
        let pairs = self
            .pairs
            .iter()
            .map(|p| {
                obj(vec![
                    ("fg", CValue::Str(p.fg.clone())),
                    ("bg", CValue::Str(p.bg.clone())),
                    ("tag", CValue::Str(p.tag.clone())),
                ])
            })
            .collect();
        obj(vec![
            ("format", CValue::Str(REPORT_FORMAT.to_string())),
            (
                "counts",
                obj(vec![
                    ("primitive", num(self.primitive)),
                    ("semantic", num(self.semantic)),
                    ("component", num(self.component)),
                    ("pairs", num(self.pairs.len())),
                    ("brand_tier", num(self.brand_tier.len())),
                ]),
            ),
            ("brand_tier", CValue::Arr(self.brand_tier.iter().map(|b| CValue::Str(b.clone())).collect())),
            ("pairs", CValue::Arr(pairs)),
            ("themes", CValue::Obj(themes)),
            ("findings", findings_value(&self.findings)),
            ("ok", CValue::Bool(self.findings.is_empty())),
        ])
    }

    /// RFC 8785 canonical JSON.
    pub fn to_json(&self) -> Result<Vec<u8>, fid_canon::CanonError> {
        fid_canon::to_canonical_bytes(&self.to_cvalue())
    }

    pub fn to_text(&self) -> String {
        let mut s = format!(
            "fid-design tokens-check: {} primitive, {} semantic, {} component colour tokens; {} pairs; brand tier {}\n",
            self.primitive,
            self.semantic,
            self.component,
            self.pairs.len(),
            self.brand_tier.len()
        );
        for t in THEMES {
            if let Some(r) = self.themes.get(t) {
                s.push_str(&format!(
                    "{t}: semantic {} resolved, {} unresolved; component {} resolved, {} unresolved\n",
                    r.semantic_resolved, r.semantic_unresolved, r.component_resolved, r.component_unresolved
                ));
            }
        }
        findings_text(&mut s, &self.findings);
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_grammar() {
        for ok in ["prim.color.blue.500", "sem.text.muted", "sem.text.value.claimed", "comp.button.bg.primary", "pal.x"]
        {
            assert_eq!(name_defect(ok), None, "{ok}");
        }
        for bad in [
            "prim.blue.500",
            "prim.color.blue",
            "sem.shade.base",
            "sem.text",
            "comp.button",
            "status.ok",
            "sem.text.a.b.c",
        ] {
            assert!(name_defect(bad).is_some(), "{bad}");
        }
    }

    #[test]
    fn codes_are_stable() {
        assert_eq!(
            Finding::new(UNRESOLVED, "dark sem.text.muted", "x").to_string(),
            "TOK-UNRESOLVED dark sem.text.muted: x"
        );
    }
}
