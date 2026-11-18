//! `cargo xtask plat-deploy netlist [--check]`: the network-feature list (spec node
//! `PLAT.DEPLOY.OFFLINE.NETLIST`, REQ-PLAT.DEPLOY.OFFLINE-005).
//!
//! Only X1, I1 and D1 commands may open sockets (CONSTITUTION §3.1, `CORE.BUS.POL.EGRESS`),
//! so the list is exactly the registry-manifest commands of those classes. Inputs are the
//! registry manifest as exported by `fid registry show-manifest --full` (`CORE.REG.MAN`) and
//! the settings manifest `settings-manifest.json` (`CORE.STO.SET`). Outputs are
//! `network-features.json` (RFC 8785, through `fid-canon`) and `network-features.md`.
//!
//! Fields read from each manifest command: `id`, `major`, `side_effect`, `exposure`,
//! `cli_bare`, `lifecycle.phase` and `opt_in_setting`. `opt_in_setting` is not yet a
//! `CORE.REG.DEF.FIELD` field; the spec's fixture fixes its name [unverified until that node
//! names it]. Where the phase lives is not fixed by `CORE.REG.MAN` either; it is read from
//! the open `lifecycle` map [unverified]. From the settings manifest only `settings[].key`
//! and `settings[].default` are read.
//!
//! The output is a pure function of the two input documents (D0).

use fid_canon::{parse, to_canonical_bytes, CValue};
use fid_types::{CommandName, SideEffect};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// Committed outputs, relative to the workspace root.
pub const DEFAULT_JSON: &str = "docs/deploy/network-features.json";
pub const DEFAULT_MD: &str = "docs/deploy/network-features.md";

/// Output format version of `network-features.json`.
pub const FORMAT: u32 = 1;

/// Errors and findings. The `Debug` form is what the CLI prints, one per line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetlistError {
    /// An X1 command declares no `opt_in_setting` (REQ-003).
    X1NoOptIn { command: String },
    /// An X1 command's setting defaults to something other than off (REQ-003).
    X1DefaultOn { command: String },
    /// An X1 command names a setting key that the settings manifest does not declare, so its
    /// default cannot be shown to be off (not named in the spec; see the PR).
    X1UnknownSetting { command: String, key: String },
    /// A manifest cannot be read or does not have the expected shape.
    ManifestParse { file: String, reason: String },
    /// `--check`: a committed output differs from a fresh generation (REQ-004).
    Drift { file: String },
}

/// One network-capable command.
#[derive(Debug, Clone, PartialEq)]
pub struct Feature {
    pub name: String,
    pub major: u32,
    pub class: SideEffect,
    pub cli: Option<String>,
    pub domain: String,
    pub phase: Option<String>,
    /// X1 only: the enabling setting key and its default.
    pub opt_in: Option<(String, CValue)>,
}

impl Feature {
    /// `name@major`.
    pub fn id(&self) -> String {
        format!("{}@{}", self.name, self.major)
    }
}

/// The two generated documents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outputs {
    pub json: Vec<u8>,
    pub md: String,
    /// Number of commands listed.
    pub count: usize,
}

fn parse_err(file: &str, reason: impl Into<String>) -> NetlistError {
    NetlistError::ManifestParse { file: file.to_string(), reason: reason.into() }
}

/// Whether a setting default counts as off: `false`, `"off"`, `"disabled"` or an empty list
/// (the egress destination list, whose default is empty).
pub fn is_off(v: &CValue) -> bool {
    match v {
        CValue::Bool(b) => !b,
        CValue::Str(s) => s == "off" || s == "disabled",
        CValue::Arr(a) => a.is_empty(),
        _ => false,
    }
}

/// The CLI name of a command per CONSTITUTION §3: `fid <domain> <verb-object>`, or
/// `fid <domain>` for a `cli_bare` command.
pub fn cli_name(name: &str, bare: bool) -> String {
    match name.split_once('.') {
        Some((domain, rest)) if !bare => format!("fid {domain} {}", rest.replace('.', " ").replace('_', "-")),
        Some((domain, _)) => format!("fid {domain}"),
        None => format!("fid {name}"),
    }
}

fn obj<'a>(v: &'a CValue, file: &str, what: &str) -> Result<&'a BTreeMap<String, CValue>, NetlistError> {
    match v {
        CValue::Obj(o) => Ok(o),
        _ => Err(parse_err(file, format!("{what} must be an object"))),
    }
}

fn array<'a>(o: &'a BTreeMap<String, CValue>, key: &str, file: &str) -> Result<&'a [CValue], NetlistError> {
    match o.get(key) {
        Some(CValue::Arr(a)) => Ok(a),
        Some(_) => Err(parse_err(file, format!("{key} must be an array"))),
        None => Err(parse_err(file, format!("missing {key}"))),
    }
}

fn string<'a>(o: &'a BTreeMap<String, CValue>, key: &str, file: &str, at: &str) -> Result<&'a str, NetlistError> {
    match o.get(key) {
        Some(CValue::Str(s)) => Ok(s),
        Some(_) => Err(parse_err(file, format!("{at}: {key} must be a string"))),
        None => Err(parse_err(file, format!("{at}: missing {key}"))),
    }
}

/// Parses the settings manifest into key -> default.
pub fn parse_settings(bytes: &[u8], file: &str) -> Result<BTreeMap<String, CValue>, NetlistError> {
    let doc = parse(bytes).map_err(|e| parse_err(file, e.code()))?;
    let top = obj(&doc, file, "the settings manifest")?;
    let mut out = BTreeMap::new();
    for (i, s) in array(top, "settings", file)?.iter().enumerate() {
        let at = format!("settings[{i}]");
        let s = obj(s, file, &at)?;
        let key = string(s, "key", file, &at)?;
        let default = s.get("default").ok_or_else(|| parse_err(file, format!("{at}: missing default")))?;
        if out.insert(key.to_string(), default.clone()).is_some() {
            return Err(parse_err(file, format!("{at}: duplicate key {key}")));
        }
    }
    Ok(out)
}

/// Reads the registry manifest and returns its X1, I1 and D1 commands sorted by command ID
/// (name, then major), each X1 command with its enabling setting and default. Every X1
/// finding is returned, in command-ID order.
pub fn collect(
    manifest: &[u8],
    manifest_file: &str,
    settings: &BTreeMap<String, CValue>,
) -> Result<Vec<Feature>, Vec<NetlistError>> {
    let file = manifest_file;
    let one = |e| vec![e];
    let doc = parse(manifest).map_err(|e| one(parse_err(file, e.code())))?;
    let top = obj(&doc, file, "the registry manifest").map_err(one)?;
    let mut features: BTreeMap<(String, u32), Feature> = BTreeMap::new();
    let mut seen: BTreeSet<(String, u32)> = BTreeSet::new();
    for (i, c) in array(top, "commands", file).map_err(one)?.iter().enumerate() {
        let at = format!("commands[{i}]");
        let c = obj(c, file, &at).map_err(one)?;
        let name = string(c, "id", file, &at).map_err(one)?;
        if !CommandName::is_valid(name) {
            return Err(one(parse_err(file, format!("{at}: bad id {name:?}"))));
        }
        let major = match c.get("major") {
            Some(CValue::Num(n)) if n.fract() == 0.0 && (0.0..=f64::from(u32::MAX)).contains(n) => *n as u32,
            _ => return Err(one(parse_err(file, format!("{at}: major must be an integer 0..2^32")))),
        };
        let class_text = string(c, "side_effect", file, &at).map_err(one)?;
        let class = SideEffect::parse(class_text)
            .map_err(|_| one(parse_err(file, format!("{at}: bad side_effect {class_text:?}"))))?;
        let key = (name.to_string(), major);
        if !seen.insert(key.clone()) {
            return Err(one(parse_err(file, format!("{at}: duplicate command {name}@{major}"))));
        }
        if !matches!(class, SideEffect::X1 | SideEffect::I1 | SideEffect::D1) {
            continue;
        }
        let has_cli = match c.get("exposure") {
            Some(CValue::Arr(a)) => a.iter().any(|s| *s == CValue::Str("cli".into())),
            _ => false,
        };
        let bare = matches!(c.get("cli_bare"), Some(CValue::Bool(true)));
        let phase = match c.get("lifecycle") {
            Some(CValue::Obj(l)) => match l.get("phase") {
                Some(CValue::Str(p)) => Some(p.clone()),
                _ => None,
            },
            _ => None,
        };
        let opt_in = match c.get("opt_in_setting") {
            None | Some(CValue::Null) => None,
            Some(CValue::Str(k)) => Some(k.clone()),
            Some(_) => return Err(one(parse_err(file, format!("{at}: opt_in_setting must be a string")))),
        };
        let domain = name.split('.').next().unwrap_or(name).to_string();
        features.insert(
            key,
            Feature {
                name: name.to_string(),
                major,
                class,
                cli: has_cli.then(|| cli_name(name, bare)),
                domain,
                phase,
                // Filled in below for X1 only.
                opt_in: opt_in.map(|k| (k, CValue::Null)),
            },
        );
    }
    let mut findings = Vec::new();
    let mut out = Vec::with_capacity(features.len());
    for mut f in features.into_values() {
        if f.class == SideEffect::X1 {
            let command = f.id();
            match f.opt_in.take() {
                None => findings.push(NetlistError::X1NoOptIn { command }),
                Some((k, _)) => match settings.get(&k) {
                    None => findings.push(NetlistError::X1UnknownSetting { command, key: k }),
                    Some(d) if !is_off(d) => findings.push(NetlistError::X1DefaultOn { command }),
                    Some(d) => f.opt_in = Some((k, d.clone())),
                },
            }
        } else {
            f.opt_in = None;
        }
        out.push(f);
    }
    if findings.is_empty() {
        Ok(out)
    } else {
        Err(findings)
    }
}

/// `network-features.json`: `{"commands": [...], "format": 1}` in RFC 8785 form.
pub fn render_json(features: &[Feature]) -> Result<Vec<u8>, NetlistError> {
    let s = |x: &str| CValue::Str(x.to_string());
    let opt = |x: &Option<String>| x.as_deref().map_or(CValue::Null, s);
    let commands = features
        .iter()
        .map(|f| {
            let mut o = BTreeMap::new();
            o.insert("id".to_string(), s(&f.id()));
            o.insert("class".to_string(), s(f.class.as_str()));
            o.insert("cli".to_string(), opt(&f.cli));
            o.insert("domain".to_string(), s(&f.domain));
            o.insert("phase".to_string(), opt(&f.phase));
            if let Some((k, d)) = &f.opt_in {
                o.insert("opt_in_setting".to_string(), s(k));
                o.insert("default".to_string(), d.clone());
            }
            CValue::Obj(o)
        })
        .collect();
    let mut top = BTreeMap::new();
    top.insert("format".to_string(), CValue::Num(f64::from(FORMAT)));
    top.insert("commands".to_string(), CValue::Arr(commands));
    let mut bytes = to_canonical_bytes(&CValue::Obj(top))
        .map_err(|e| parse_err("network-features.json", format!("cannot serialise: {}", e.code())))?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn cell(s: &str) -> String {
    s.replace('|', "\\|")
}

/// `network-features.md`: generated header, the I1/D1 statement and one table row per command.
pub fn render_md(features: &[Feature]) -> Result<String, NetlistError> {
    let mut md = String::from(
        "<!-- Generated by `cargo xtask plat-deploy netlist` (spec node PLAT.DEPLOY.OFFLINE.NETLIST). \
         Do not edit: `cargo xtask plat-deploy netlist --check` fails when this file differs from a fresh generation. -->\n\
         \n\
         # Network features\n\
         \n\
         Only commands of side-effect class X1 (egress), I1 (simulator I/O) and D1 (device I/O) may open network \
         sockets (CONSTITUTION section 3.1, CORE.BUS.POL.EGRESS). This list is generated from the registry manifest \
         and the settings manifest, and names every such command.\n\
         \n\
         Every X1 command is off by default: its opt-in setting below defaults to off (`false`, `off`, `disabled` \
         or an empty list), and generation fails otherwise.\n\
         \n\
         I1 and D1 commands reach only the simulators and devices that have been configured, and run only on \
         explicit user action.\n\
         \n",
    );
    if features.is_empty() {
        md.push_str("_No command in the registry manifest can use the network._\n");
        return Ok(md);
    }
    md.push_str("| Command | Class | CLI | Domain | Phase | Opt-in setting | Default |\n");
    md.push_str("|---|---|---|---|---|---|---|\n");
    for f in features {
        let code = |x: &str| format!("`{}`", cell(x));
        let cli = f.cli.as_deref().map_or_else(|| "n/a".to_string(), code);
        let phase = f.phase.as_deref().map_or_else(|| "n/a".to_string(), cell);
        let (key, default) = match &f.opt_in {
            Some((k, d)) => {
                let d = to_canonical_bytes(d)
                    .ok()
                    .and_then(|b| String::from_utf8(b).ok())
                    .ok_or_else(|| parse_err("network-features.md", "cannot serialise a default"))?;
                (code(k), code(&d))
            }
            None => ("n/a".to_string(), "n/a".to_string()),
        };
        md.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} |\n",
            code(&f.id()),
            f.class.as_str(),
            cli,
            cell(&f.domain),
            phase,
            key,
            default
        ));
    }
    let n = features.len();
    md.push_str(&format!("\n{n} command{}.\n", if n == 1 { "" } else { "s" }));
    Ok(md)
}

/// Generates both documents from the two manifests' bytes.
pub fn generate(
    manifest: &[u8],
    manifest_file: &str,
    settings: &[u8],
    settings_file: &str,
) -> Result<Outputs, Vec<NetlistError>> {
    let settings = parse_settings(settings, settings_file).map_err(|e| vec![e])?;
    let features = collect(manifest, manifest_file, &settings)?;
    let json = render_json(&features).map_err(|e| vec![e])?;
    let md = render_md(&features).map_err(|e| vec![e])?;
    Ok(Outputs { json, md, count: features.len() })
}

fn read(path: &Path) -> Result<Vec<u8>, NetlistError> {
    std::fs::read(path).map_err(|e| parse_err(&path.display().to_string(), format!("cannot read: {e}")))
}

/// Reads the manifests and generates. Then, with `check`, compares the result with the files at
/// `json_out` and `md_out` (a missing or different file is `Drift`); otherwise writes them.
pub fn run(
    manifest: &Path,
    settings: &Path,
    json_out: &Path,
    md_out: &Path,
    check: bool,
) -> Result<usize, Vec<NetlistError>> {
    let m = read(manifest).map_err(|e| vec![e])?;
    let s = read(settings).map_err(|e| vec![e])?;
    let out = generate(&m, &manifest.display().to_string(), &s, &settings.display().to_string())?;
    let count = out.count;
    let targets = [(json_out, out.json), (md_out, out.md.into_bytes())];
    if check {
        let drift: Vec<NetlistError> = targets
            .iter()
            .filter(|(p, want)| std::fs::read(p).ok().as_ref() != Some(want))
            .map(|(p, _)| NetlistError::Drift { file: p.display().to_string() })
            .collect();
        return if drift.is_empty() { Ok(count) } else { Err(drift) };
    }
    for (p, bytes) in &targets {
        if let Some(dir) = p.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir).map_err(|e| vec![parse_err(&dir.display().to_string(), e.to_string())])?;
        }
        std::fs::write(p, bytes)
            .map_err(|e| vec![parse_err(&p.display().to_string(), format!("cannot write: {e}"))])?;
    }
    Ok(count)
}
