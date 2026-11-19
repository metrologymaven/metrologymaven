//! `shaders/manifest.toml`: every WGSL module of the renderer with its bindings classified as
//! input data, colour target, depth target or ID target (REQ-VIZ.RENDER.FRAME.SHLINT-001).
//!
//! ```toml
//! schema = 1
//!
//! [[module]]
//! path = "shaders/points.wgsl"      # relative to the crate directory, `/` separators
//!
//! [[module.binding]]
//! group = 0
//! binding = 1
//! name = "points"                   # the WGSL variable name
//! class = "input"                   # input | colour | depth | id
//!
//! [[target]]                        # optional: a read-back target that is not a shader binding,
//! name = "id_readback"              # e.g. the buffer a colour, depth or ID attachment is copied to
//! class = "id"                      # colour | depth | id
//! ```
//!
//! Parsing fails closed: unknown keys, wrong types, duplicate modules, slots, names or keys,
//! unsafe paths and unknown classes are all [`crate::ShlintError::ManifestMalformed`].

use std::collections::BTreeSet;

/// Classification of a binding or read-back target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum BindingClass {
    Input,
    Colour,
    Depth,
    Id,
}

impl BindingClass {
    /// Parses the manifest spelling (`input`, `colour`, `depth`, `id`).
    pub fn parse(s: &str) -> Option<BindingClass> {
        match s {
            "input" => Some(BindingClass::Input),
            "colour" => Some(BindingClass::Colour),
            "depth" => Some(BindingClass::Depth),
            "id" => Some(BindingClass::Id),
            _ => None,
        }
    }

    /// The manifest spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            BindingClass::Input => "input",
            BindingClass::Colour => "colour",
            BindingClass::Depth => "depth",
            BindingClass::Id => "id",
        }
    }

    /// True for colour, depth and ID targets: the only things non-test code may read back.
    pub fn is_target(self) -> bool {
        !matches!(self, BindingClass::Input)
    }
}

/// One resource binding of a module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    pub group: u32,
    pub binding: u32,
    pub name: String,
    pub class: BindingClass,
}

/// One WGSL module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Module {
    pub path: String,
    pub bindings: Vec<Binding>,
}

/// A read-back target that is not a shader binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub name: String,
    pub class: BindingClass,
}

/// How the manifest classifies a name that Rust code maps for reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadbackClass {
    /// Every entry with this name is a colour, depth or ID target.
    Target,
    /// At least one entry with this name is input data.
    Input,
    /// No entry has this name.
    Unknown,
}

/// The parsed manifest (`ShaderManifest { modules, targets }`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ShaderManifest {
    pub modules: Vec<Module>,
    pub targets: Vec<Target>,
}

type Table = toml::Table;

impl ShaderManifest {
    /// Parses and validates the manifest text.
    pub fn parse(text: &str) -> Result<ShaderManifest, String> {
        let table: Table = text.parse().map_err(|e: toml::de::Error| e.to_string().trim_end().to_string())?;
        only_keys(&table, &["schema", "module", "target"], "the top level")?;
        match table.get("schema") {
            Some(toml::Value::Integer(1)) => {}
            Some(_) => return Err("`schema` must be the integer 1".to_string()),
            None => return Err("`schema = 1` is missing".to_string()),
        }
        let mut out = ShaderManifest::default();
        let mut paths = BTreeSet::new();
        for (i, t) in tables(&table, "module")?.into_iter().enumerate() {
            let at = format!("module #{}", i + 1);
            only_keys(t, &["path", "binding"], &at)?;
            let path = string(t, "path", &at)?;
            check_path(&path).map_err(|r| format!("{at}: path `{path}` {r}"))?;
            if !paths.insert(path.clone()) {
                return Err(format!("{at}: module {path} is listed twice"));
            }
            let at = format!("module {path}");
            let mut slots = BTreeSet::new();
            let mut names = BTreeSet::new();
            let mut bindings = Vec::new();
            for (j, b) in tables(t, "binding")?.into_iter().enumerate() {
                let bat = format!("{at} binding #{}", j + 1);
                only_keys(b, &["group", "binding", "name", "class"], &bat)?;
                let group = u32_field(b, "group", &bat)?;
                let binding = u32_field(b, "binding", &bat)?;
                let name = string(b, "name", &bat)?;
                check_ident(&name).map_err(|r| format!("{bat}: name `{name}` {r}"))?;
                let class = class_field(b, &bat)?;
                if !slots.insert((group, binding)) {
                    return Err(format!("{at}: @group({group}) @binding({binding}) is listed twice"));
                }
                if !names.insert(name.clone()) {
                    return Err(format!("{at}: binding name `{name}` is listed twice"));
                }
                bindings.push(Binding { group, binding, name, class });
            }
            out.modules.push(Module { path, bindings });
        }
        let mut target_names = BTreeSet::new();
        for (i, t) in tables(&table, "target")?.into_iter().enumerate() {
            let at = format!("target #{}", i + 1);
            only_keys(t, &["name", "class"], &at)?;
            let name = string(t, "name", &at)?;
            check_ident(&name).map_err(|r| format!("{at}: name `{name}` {r}"))?;
            let class = class_field(t, &at)?;
            if !class.is_target() {
                return Err(format!("{at}: class of a read-back target must be colour, depth or id"));
            }
            if !target_names.insert(name.clone()) {
                return Err(format!("{at}: target `{name}` is listed twice"));
            }
            out.targets.push(Target { name, class });
        }
        Ok(out)
    }

    /// The module listed under `path`, if any.
    pub fn module(&self, path: &str) -> Option<&Module> {
        self.modules.iter().find(|m| m.path == path)
    }

    /// How a name mapped for reading is classified across all bindings and targets.
    pub fn readback_class(&self, name: &str) -> ReadbackClass {
        let classes = self
            .modules
            .iter()
            .flat_map(|m| m.bindings.iter())
            .filter(|b| b.name == name)
            .map(|b| b.class)
            .chain(self.targets.iter().filter(|t| t.name == name).map(|t| t.class));
        let mut any = false;
        for c in classes {
            if !c.is_target() {
                return ReadbackClass::Input;
            }
            any = true;
        }
        if any {
            ReadbackClass::Target
        } else {
            ReadbackClass::Unknown
        }
    }
}

fn only_keys(t: &Table, allowed: &[&str], at: &str) -> Result<(), String> {
    match t.keys().find(|k| !allowed.contains(&k.as_str())) {
        Some(k) => Err(format!("{at}: unknown key `{k}`")),
        None => Ok(()),
    }
}

/// An optional array of tables; absent means empty.
fn tables<'a>(t: &'a Table, key: &str) -> Result<Vec<&'a Table>, String> {
    match t.get(key) {
        None => Ok(Vec::new()),
        Some(toml::Value::Array(items)) => items
            .iter()
            .map(|v| match v {
                toml::Value::Table(t) => Ok(t),
                _ => Err(format!("`{key}` must be an array of tables")),
            })
            .collect(),
        Some(_) => Err(format!("`{key}` must be an array of tables")),
    }
}

fn string(t: &Table, key: &str, at: &str) -> Result<String, String> {
    match t.get(key) {
        Some(toml::Value::String(s)) => Ok(s.clone()),
        Some(_) => Err(format!("{at}: `{key}` must be a string")),
        None => Err(format!("{at}: `{key}` is missing")),
    }
}

fn u32_field(t: &Table, key: &str, at: &str) -> Result<u32, String> {
    match t.get(key) {
        Some(toml::Value::Integer(n)) => {
            u32::try_from(*n).map_err(|_| format!("{at}: `{key}` must be an integer from 0 to {}", u32::MAX))
        }
        Some(_) => Err(format!("{at}: `{key}` must be an integer")),
        None => Err(format!("{at}: `{key}` is missing")),
    }
}

fn class_field(t: &Table, at: &str) -> Result<BindingClass, String> {
    let s = string(t, "class", at)?;
    BindingClass::parse(&s).ok_or_else(|| format!("{at}: class `{s}` is not one of input, colour, depth, id"))
}

/// A safe relative `.wgsl` path with `/` separators.
fn check_path(p: &str) -> Result<(), &'static str> {
    if p.is_empty() || p.starts_with('/') || p.contains('\\') || p.contains(':') {
        return Err("must be relative with `/` separators");
    }
    if p.split('/').any(|s| s.is_empty() || s == "." || s == "..") {
        return Err("must not contain empty, `.` or `..` segments");
    }
    if !p.ends_with(".wgsl") {
        return Err("must end in .wgsl");
    }
    Ok(())
}

/// An ASCII identifier: a letter or `_`, then letters, digits or `_`.
fn check_ident(s: &str) -> Result<(), &'static str> {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return Err("must start with an ASCII letter or `_`"),
    }
    if chars.all(|c| c.is_ascii_alphanumeric() || c == '_') {
        Ok(())
    } else {
        Err("must contain only ASCII letters, digits and `_`")
    }
}
