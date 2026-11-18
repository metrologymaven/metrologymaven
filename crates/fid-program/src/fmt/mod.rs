//! Command Program document format. Spec node `AUTO.PROG.FMT`.
//!
//! A program is stored as RFC 8785 canonical JSON (through `fid-canon`), so one program
//! has exactly one byte form and one digest. Parsing is the only way in, and it fails
//! closed: size, nesting, value and step counts are bounded before or while the tree is
//! built, unknown fields are refused, and every step carries a UUIDv7 that is unique
//! within the program. Expressions, parameter declarations and operator descriptors are
//! kept as source text or opaque objects for `AUTO.PROG.EXPR`, `AUTO.PROG.FAMILY` and
//! `AUTO.PROG.OPER`.

mod shape;
mod uuid;

pub use shape::{program_schema, COMMAND_PATTERN, IDENT_PATTERN, MAX_MAJOR, UUID7_PATTERN};
pub use uuid::Uuid7;

use fid_canon::{digest_bytes, format_number, parse_with_limits, to_canonical_bytes, CValue, CanonError};
use fid_types::Digest;
use shape::{Shape, Ty};
use std::collections::{BTreeMap, BTreeSet};

/// The only `format` value.
pub const FORMAT: &str = "fid-program";
/// The only supported `format_version`.
pub const FORMAT_VERSION: u32 = 1;
/// Largest accepted document, in bytes (REQ-AUTO.PROG.FMT-006).
pub const MAX_BYTES: usize = 16 * 1024 * 1024;
/// Deepest accepted nesting of JSON arrays and objects (REQ-AUTO.PROG.FMT-006).
pub const MAX_DEPTH: usize = 64;
/// Most steps in one program, counting nested and subroutine steps (REQ-AUTO.PROG.FMT-006).
pub const MAX_STEPS: usize = 100_000;
/// Most JSON structural tokens (`[`, `{`, `,`, `:`), checked before the tree is built so the
/// in-memory tree stays bounded.
pub const MAX_TOKENS: usize = 2_000_000;
/// Most JSON objects, checked before the tree is built (objects cost the most memory).
pub const MAX_OBJECTS: usize = 250_000;

/// Opaque JSON object (meta, parameter declaration, operator descriptor).
pub type Opaque = BTreeMap<String, CValue>;

/// Which bound a document exceeded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bound {
    Bytes,
    Depth,
    Tokens,
    Objects,
    Steps,
}

impl Bound {
    /// Stable name.
    pub fn as_str(self) -> &'static str {
        match self {
            Bound::Bytes => "bytes",
            Bound::Depth => "depth",
            Bound::Tokens => "tokens",
            Bound::Objects => "objects",
            Bound::Steps => "steps",
        }
    }
}

/// Program format errors with stable codes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProgramError {
    /// `format_version` is a number other than 1; holds its canonical text.
    FormatVersionUnsupported(String),
    /// The bytes are JSON but not the canonical form of themselves.
    NonCanonical,
    /// Two steps share this ID.
    DuplicateStepId(Uuid7),
    /// An object carries a field its shape does not declare. `path` is a JSON Pointer.
    UnknownField { path: String, field: String },
    /// Any other schema violation. `path` is a JSON Pointer.
    SchemaInvalid { path: String, reason: String },
    /// A bound of REQ-AUTO.PROG.FMT-006 was exceeded.
    ProgramTooLarge(Bound),
    /// Not JSON at all (invalid UTF-8, syntax, duplicate key); holds the `fid-canon` code.
    InvalidJson(&'static str),
}

impl ProgramError {
    /// Stable machine-readable code.
    pub fn code(&self) -> &'static str {
        match self {
            ProgramError::FormatVersionUnsupported(_) => "format_version_unsupported",
            ProgramError::NonCanonical => "non_canonical",
            ProgramError::DuplicateStepId(_) => "duplicate_step_id",
            ProgramError::UnknownField { .. } => "unknown_field",
            ProgramError::SchemaInvalid { .. } => "schema_invalid",
            ProgramError::ProgramTooLarge(_) => "program_too_large",
            ProgramError::InvalidJson(_) => "invalid_json",
        }
    }
}

impl std::fmt::Display for ProgramError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProgramError::FormatVersionUnsupported(v) => write!(f, "format_version_unsupported: {v}"),
            ProgramError::DuplicateStepId(id) => write!(f, "duplicate_step_id: {id}"),
            ProgramError::UnknownField { path, field } => write!(f, "unknown_field: {field:?} at {path:?}"),
            ProgramError::SchemaInvalid { path, reason } => write!(f, "schema_invalid: {reason} at {path:?}"),
            ProgramError::ProgramTooLarge(b) => write!(f, "program_too_large: {}", b.as_str()),
            ProgramError::InvalidJson(c) => write!(f, "invalid_json: {c}"),
            ProgramError::NonCanonical => f.write_str("non_canonical"),
        }
    }
}

impl std::error::Error for ProgramError {}

// ------------------------------------------------------------------ model

/// A Command Program, format version 1.
#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub format_version: u32,
    pub program_id: Uuid7,
    /// Free metadata (title, author, notes); opaque here.
    pub meta: Opaque,
    /// Part-family parameter declarations; validated by `AUTO.PROG.FAMILY`.
    pub params: Vec<Opaque>,
    pub variables: Vec<VarDecl>,
    pub subroutines: Vec<Subroutine>,
    pub main: Vec<Step>,
}

/// A typed variable or subroutine parameter. `ty` is type source for `AUTO.PROG.EXPR`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VarDecl {
    pub name: String,
    pub ty: String,
}

/// A subroutine with typed parameters and an optional return type.
#[derive(Debug, Clone, PartialEq)]
pub struct Subroutine {
    pub name: String,
    pub params: Vec<VarDecl>,
    pub returns: Option<String>,
    pub body: Vec<Step>,
}

/// One step: a stable ID and what it does.
#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    pub step_id: Uuid7,
    pub kind: StepKind,
}

/// A command or call argument.
#[derive(Debug, Clone, PartialEq)]
pub enum Arg {
    /// `{"literal": <JSON>}`: a recorded value, stored canonically.
    Literal(CValue),
    /// `{"expr": "<source>"}`: expression source for `AUTO.PROG.EXPR`.
    Expr(String),
}

/// One `if` / `else if` branch.
#[derive(Debug, Clone, PartialEq)]
pub struct Branch {
    pub cond: String,
    pub body: Vec<Step>,
}

/// Loop form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoopMode {
    Counted { var: String, from: String, to: String, step: String },
    While { cond: String },
}

/// The closed set of step kinds. `Invoke` is the only one that names a state-changing
/// action (REQ-AUTO.PROG.FMT-004).
#[derive(Debug, Clone, PartialEq)]
pub enum StepKind {
    Invoke { command: String, major: u32, args: BTreeMap<String, Arg>, bind: Option<String> },
    Assign { var: String, expr: String },
    If { branches: Vec<Branch>, else_body: Vec<Step> },
    Loop { mode: LoopMode, max_iterations: u32, body: Vec<Step> },
    Call { subroutine: String, args: BTreeMap<String, Arg>, bind: Option<String> },
    Return { expr: Option<String> },
    Label { name: String },
    Jump { label: String, cond: Option<String> },
    Operator { descriptor: Opaque },
    Comment { text: String },
}

impl StepKind {
    /// The wire `kind`.
    pub fn as_str(&self) -> &'static str {
        match self {
            StepKind::Invoke { .. } => "invoke",
            StepKind::Assign { .. } => "assign",
            StepKind::If { .. } => "if",
            StepKind::Loop { .. } => "loop",
            StepKind::Call { .. } => "call",
            StepKind::Return { .. } => "return",
            StepKind::Label { .. } => "label",
            StepKind::Jump { .. } => "jump",
            StepKind::Operator { .. } => "operator",
            StepKind::Comment { .. } => "comment",
        }
    }
}

impl Program {
    /// Parses stored bytes. They must be the canonical form of a valid program
    /// (REQ-AUTO.PROG.FMT-001..006).
    pub fn parse(bytes: &[u8]) -> Result<Program, ProgramError> {
        if bytes.len() > MAX_BYTES {
            return Err(ProgramError::ProgramTooLarge(Bound::Bytes));
        }
        prescan(bytes)?;
        let value = parse_with_limits(bytes, MAX_BYTES, MAX_DEPTH).map_err(|e| match e {
            CanonError::InputLimit => ProgramError::ProgramTooLarge(Bound::Depth),
            other => ProgramError::InvalidJson(other.code()),
        })?;
        check_header(&value)?;
        let canonical = to_canonical_bytes(&value).map_err(|e| ProgramError::InvalidJson(e.code()))?;
        if canonical.as_slice() != bytes {
            return Err(ProgramError::NonCanonical);
        }
        drop(canonical);
        let mut dec = Decoder::default();
        let program = dec.program(value)?;
        // The decoder is lossless by construction; this guards the invariant of REQ-002.
        // The encoder writes from the program itself, so no second tree is built.
        if program.encode_bytes()?.as_slice() != bytes {
            return Err(ProgramError::NonCanonical);
        }
        Ok(program)
    }

    /// The stored (canonical) bytes. Applies the rules of [`Program::parse`] to the
    /// in-memory program first, so everything this returns parses back to an equal program.
    pub fn to_canonical(&self) -> Result<Vec<u8>, ProgramError> {
        Checker::default().program(self)?;
        let bytes = self.encode_bytes()?;
        if bytes.len() > MAX_BYTES {
            return Err(ProgramError::ProgramTooLarge(Bound::Bytes));
        }
        prescan(&bytes)?;
        Ok(bytes)
    }

    /// SHA-256 of the canonical bytes, as `sha256:<hex>` (REQ-AUTO.PROG.FMT-005).
    pub fn digest(&self) -> Result<Digest, ProgramError> {
        self.to_canonical().map(|b| Digest::from_bytes(digest_bytes(&b).0))
    }

    /// Every step ID in document order (main, then subroutines; depth first).
    pub fn step_ids(&self) -> Vec<Uuid7> {
        fn walk(steps: &[Step], out: &mut Vec<Uuid7>) {
            for s in steps {
                out.push(s.step_id);
                match &s.kind {
                    StepKind::If { branches, else_body } => {
                        for b in branches {
                            walk(&b.body, out);
                        }
                        walk(else_body, out);
                    }
                    StepKind::Loop { body, .. } => walk(body, out),
                    _ => {}
                }
            }
        }
        let mut out = Vec::new();
        walk(&self.main, &mut out);
        for sub in &self.subroutines {
            walk(&sub.body, &mut out);
        }
        out
    }

    fn encode_bytes(&self) -> Result<Vec<u8>, ProgramError> {
        to_canonical_bytes(&self.encode()?).map_err(|e| ProgramError::InvalidJson(e.code()))
    }
}

/// Counts structural tokens, objects and nesting outside strings, so hostile input cannot
/// build a huge tree (REQ-AUTO.PROG.FMT-006, AC-6). Also bounds encoded output.
fn prescan(bytes: &[u8]) -> Result<(), ProgramError> {
    let (mut tokens, mut objects, mut depth) = (0usize, 0usize, 0usize);
    let (mut in_str, mut escaped) = (false, false);
    for &c in bytes {
        if in_str {
            if escaped {
                escaped = false;
            } else if c == b'\\' {
                escaped = true;
            } else if c == b'"' {
                in_str = false;
            }
            continue;
        }
        match c {
            b'"' => in_str = true,
            b'{' | b'[' => {
                tokens += 1;
                depth += 1;
                if depth > MAX_DEPTH {
                    return Err(ProgramError::ProgramTooLarge(Bound::Depth));
                }
                if c == b'{' {
                    objects += 1;
                    if objects > MAX_OBJECTS {
                        return Err(ProgramError::ProgramTooLarge(Bound::Objects));
                    }
                }
            }
            b'}' | b']' => {
                depth = depth.saturating_sub(1);
                continue;
            }
            b',' | b':' => tokens += 1,
            _ => continue,
        }
        if tokens > MAX_TOKENS {
            return Err(ProgramError::ProgramTooLarge(Bound::Tokens));
        }
    }
    Ok(())
}

/// `format` and `format_version` are checked before anything else, so a document of
/// another version is refused by version, not by whatever else changed.
fn check_header(v: &CValue) -> Result<(), ProgramError> {
    let CValue::Obj(map) = v else {
        return Err(invalid(&ROOT, "a program must be a JSON object"));
    };
    if !matches!(map.get("format"), Some(CValue::Str(f)) if f == FORMAT) {
        return Err(invalid(&ROOT.key("format"), format!("must be {FORMAT:?}")));
    }
    match map.get("format_version") {
        Some(n) if shape::as_u32(n) == Some(FORMAT_VERSION) => Ok(()),
        Some(CValue::Num(n)) => {
            Err(ProgramError::FormatVersionUnsupported(format_number(*n).unwrap_or_else(|_| String::from("?"))))
        }
        _ => Err(invalid(&ROOT.key("format_version"), "must be an integer")),
    }
}

// ------------------------------------------------------------------ paths

#[derive(Clone, Copy)]
enum Seg<'a> {
    Root,
    Key(&'a str),
    Idx(usize),
}

/// A JSON Pointer built lazily: rendered only when an error is reported.
#[derive(Clone, Copy)]
struct At<'a> {
    parent: Option<&'a At<'a>>,
    seg: Seg<'a>,
}

const ROOT: At<'static> = At { parent: None, seg: Seg::Root };

impl<'a> At<'a> {
    fn key(&'a self, k: &'a str) -> At<'a> {
        At { parent: Some(self), seg: Seg::Key(k) }
    }

    fn idx(&'a self, i: usize) -> At<'a> {
        At { parent: Some(self), seg: Seg::Idx(i) }
    }

    fn render(&self) -> String {
        let mut segs = Vec::new();
        let mut cur = Some(self);
        while let Some(a) = cur {
            segs.push(a.seg);
            cur = a.parent;
        }
        let mut out = String::new();
        for seg in segs.iter().rev() {
            match seg {
                Seg::Root => {}
                Seg::Key(k) => {
                    out.push('/');
                    out.push_str(&k.replace('~', "~0").replace('/', "~1"));
                }
                Seg::Idx(i) => {
                    out.push('/');
                    out.push_str(&i.to_string());
                }
            }
        }
        out
    }
}

fn invalid(at: &At<'_>, reason: impl Into<String>) -> ProgramError {
    ProgramError::SchemaInvalid { path: at.render(), reason: reason.into() }
}

// ------------------------------------------------------------------ decoding

/// An object checked against its shape: no unknown field, every required field present,
/// every present field of the declared type.
struct Fields<'a> {
    map: Opaque,
    at: &'a At<'a>,
}

fn open<'a>(v: CValue, shape: &Shape, at: &'a At<'a>) -> Result<Fields<'a>, ProgramError> {
    let CValue::Obj(map) = v else {
        return Err(invalid(at, "must be an object"));
    };
    if let Some(k) = map.keys().find(|k| !shape.fields.iter().any(|f| f.name == k.as_str())) {
        return Err(ProgramError::UnknownField { path: at.render(), field: k.clone() });
    }
    for f in shape.fields {
        match map.get(f.name) {
            None if f.required => return Err(invalid(at, format!("missing field {:?}", f.name))),
            Some(v) if !f.ty.admits(v) => return Err(invalid(&at.key(f.name), format!("must be {}", f.ty.expected()))),
            _ => {}
        }
    }
    Ok(Fields { map, at })
}

impl Fields<'_> {
    fn take(&mut self, k: &'static str) -> Result<CValue, ProgramError> {
        self.map.remove(k).ok_or_else(|| invalid(self.at, format!("missing field {k:?}")))
    }

    fn text(&mut self, k: &'static str) -> Result<String, ProgramError> {
        match self.take(k)? {
            CValue::Str(s) => Ok(s),
            _ => Err(invalid(&self.at.key(k), "must be a string")),
        }
    }

    fn opt_text(&mut self, k: &'static str) -> Result<Option<String>, ProgramError> {
        if self.map.contains_key(k) {
            self.text(k).map(Some)
        } else {
            Ok(None)
        }
    }

    fn u32(&mut self, k: &'static str) -> Result<u32, ProgramError> {
        let v = self.take(k)?;
        shape::as_u32(&v).ok_or_else(|| invalid(&self.at.key(k), "must be an integer"))
    }

    fn uuid(&mut self, k: &'static str) -> Result<Uuid7, ProgramError> {
        let s = self.text(k)?;
        Uuid7::parse(&s).ok_or_else(|| invalid(&self.at.key(k), "must be a lowercase hyphenated UUIDv7"))
    }

    fn object(&mut self, k: &'static str) -> Result<Opaque, ProgramError> {
        match self.take(k)? {
            CValue::Obj(m) => Ok(m),
            _ => Err(invalid(&self.at.key(k), "must be an object")),
        }
    }

    fn array(&mut self, k: &'static str) -> Result<Vec<CValue>, ProgramError> {
        match self.take(k)? {
            CValue::Arr(items) => Ok(items),
            _ => Err(invalid(&self.at.key(k), "must be an array")),
        }
    }
}

#[derive(Default)]
struct Decoder {
    count: usize,
    ids: BTreeSet<Uuid7>,
}

impl Decoder {
    fn program(&mut self, v: CValue) -> Result<Program, ProgramError> {
        let root = ROOT;
        let mut f = open(v, &shape::PROGRAM, &root)?;
        let format_version = f.u32("format_version")?;
        let program_id = f.uuid("program_id")?;
        let meta = f.object("meta")?;
        let at_p = root.key("params");
        let mut params = Vec::new();
        for (i, item) in f.array("params")?.into_iter().enumerate() {
            match item {
                CValue::Obj(m) => params.push(m),
                _ => return Err(invalid(&at_p.idx(i), "must be an object")),
            }
        }
        let at_v = root.key("variables");
        let mut variables = Vec::new();
        for (i, item) in f.array("variables")?.into_iter().enumerate() {
            variables.push(var_decl(item, &at_v.idx(i))?);
        }
        // Main first, then subroutines: the canonical byte order of the two keys.
        let main = self.steps(f.array("main")?, &root.key("main"))?;
        let at_s = root.key("subroutines");
        let mut subroutines = Vec::new();
        for (i, item) in f.array("subroutines")?.into_iter().enumerate() {
            subroutines.push(self.subroutine(item, &at_s.idx(i))?);
        }
        Ok(Program { format_version, program_id, meta, params, variables, subroutines, main })
    }

    fn subroutine(&mut self, v: CValue, at: &At<'_>) -> Result<Subroutine, ProgramError> {
        let mut f = open(v, &shape::SUBROUTINE, at)?;
        let name = f.text("name")?;
        let at_p = at.key("params");
        let mut params = Vec::new();
        for (i, item) in f.array("params")?.into_iter().enumerate() {
            params.push(var_decl(item, &at_p.idx(i))?);
        }
        let returns = f.opt_text("returns")?;
        let body = self.steps(f.array("body")?, &at.key("body"))?;
        Ok(Subroutine { name, params, returns, body })
    }

    fn steps(&mut self, items: Vec<CValue>, at: &At<'_>) -> Result<Vec<Step>, ProgramError> {
        // Refuse before allocating: `[0,0,...]` must not reserve room for millions of steps.
        if self.count.saturating_add(items.len()) > MAX_STEPS {
            return Err(ProgramError::ProgramTooLarge(Bound::Steps));
        }
        let mut out = Vec::with_capacity(items.len().min(MAX_STEPS - self.count));
        for (i, item) in items.into_iter().enumerate() {
            out.push(self.step(item, &at.idx(i))?);
        }
        Ok(out)
    }

    fn step(&mut self, v: CValue, at: &At<'_>) -> Result<Step, ProgramError> {
        self.count += 1;
        if self.count > MAX_STEPS {
            return Err(ProgramError::ProgramTooLarge(Bound::Steps));
        }
        let (kind, counted, shape) = {
            let CValue::Obj(map) = &v else {
                return Err(invalid(at, "a step must be an object"));
            };
            let Some(CValue::Str(kind)) = map.get("kind") else {
                return Err(invalid(&at.key("kind"), "must be a step kind"));
            };
            let mode = match map.get("mode") {
                Some(CValue::Str(m)) => Some(m.as_str()),
                _ => None,
            };
            let Some(shape) = shape::step_shape(kind, mode) else {
                return Err(invalid(at, format!("unknown step kind {kind:?} (or loop mode)")));
            };
            (kind.clone(), mode == Some("counted"), shape)
        };
        let mut f = open(v, shape, at)?;
        let step_id = f.uuid("step_id")?;
        if !self.ids.insert(step_id) {
            return Err(ProgramError::DuplicateStepId(step_id));
        }
        let kind = match kind.as_str() {
            "invoke" => StepKind::Invoke {
                command: f.text("command")?,
                major: f.u32("major")?,
                args: args(f.object("args")?, &at.key("args"))?,
                bind: f.opt_text("bind")?,
            },
            "assign" => StepKind::Assign { var: f.text("var")?, expr: f.text("expr")? },
            "if" => {
                let at_b = at.key("branches");
                let mut branches = Vec::new();
                for (i, item) in f.array("branches")?.into_iter().enumerate() {
                    let at_i = at_b.idx(i);
                    let mut b = open(item, &shape::BRANCH, &at_i)?;
                    let cond = b.text("cond")?;
                    let body = self.steps(b.array("body")?, &at_i.key("body"))?;
                    branches.push(Branch { cond, body });
                }
                let else_body = self.steps(f.array("else_body")?, &at.key("else_body"))?;
                StepKind::If { branches, else_body }
            }
            "loop" => {
                let mode = if counted {
                    LoopMode::Counted {
                        var: f.text("var")?,
                        from: f.text("from")?,
                        to: f.text("to")?,
                        step: f.text("step")?,
                    }
                } else {
                    LoopMode::While { cond: f.text("cond")? }
                };
                let max_iterations = f.u32("max_iterations")?;
                let body = self.steps(f.array("body")?, &at.key("body"))?;
                StepKind::Loop { mode, max_iterations, body }
            }
            "call" => StepKind::Call {
                subroutine: f.text("subroutine")?,
                args: args(f.object("args")?, &at.key("args"))?,
                bind: f.opt_text("bind")?,
            },
            "return" => StepKind::Return { expr: f.opt_text("expr")? },
            "label" => StepKind::Label { name: f.text("name")? },
            "jump" => StepKind::Jump { label: f.text("label")?, cond: f.opt_text("cond")? },
            "operator" => StepKind::Operator { descriptor: f.object("descriptor")? },
            "comment" => StepKind::Comment { text: f.text("text")? },
            other => return Err(invalid(at, format!("unknown step kind {other:?}"))),
        };
        Ok(Step { step_id, kind })
    }
}

fn var_decl(v: CValue, at: &At<'_>) -> Result<VarDecl, ProgramError> {
    let mut f = open(v, &shape::VAR_DECL, at)?;
    Ok(VarDecl { name: f.text("name")?, ty: f.text("type")? })
}

fn args(map: Opaque, at: &At<'_>) -> Result<BTreeMap<String, Arg>, ProgramError> {
    let mut out = BTreeMap::new();
    for (name, v) in map {
        let at_n = at.key(&name);
        let shape = match &v {
            CValue::Obj(m) if m.contains_key("literal") => &shape::ARG_LITERAL,
            CValue::Obj(m) if m.contains_key("expr") => &shape::ARG_EXPR,
            _ => return Err(invalid(&at_n, "an argument must be {\"literal\": <JSON>} or {\"expr\": \"<source>\"}")),
        };
        let mut f = open(v, shape, &at_n)?;
        let arg =
            if f.map.contains_key("literal") { Arg::Literal(f.take("literal")?) } else { Arg::Expr(f.text("expr")?) };
        out.insert(name, arg);
    }
    Ok(out)
}

// ------------------------------------------------------------------ checking

/// Deepest step nesting the checker and encoder walk; deeper programs cannot be stored
/// anyway (each level costs at least two JSON levels of [`MAX_DEPTH`]).
const MAX_STEP_NESTING: usize = MAX_DEPTH / 2;

fn field_ty(shape: &Shape, name: &str) -> Ty {
    shape.fields.iter().find(|f| f.name == name).map_or(Ty::Json, |f| f.ty)
}

/// Checks a string field of an in-memory program against the field table.
fn want_str(shape: &Shape, name: &'static str, v: &str, at: &At<'_>) -> Result<(), ProgramError> {
    let ty = field_ty(shape, name);
    if ty.admits_str(v) {
        Ok(())
    } else {
        Err(invalid(&at.key(name), format!("must be {}", ty.expected())))
    }
}

fn want_u32(shape: &Shape, name: &'static str, v: u32, at: &At<'_>) -> Result<(), ProgramError> {
    let ty = field_ty(shape, name);
    if ty.admits_u32(v) {
        Ok(())
    } else {
        Err(invalid(&at.key(name), format!("must be {}", ty.expected())))
    }
}

fn want_args(args: &BTreeMap<String, Arg>, at: &At<'_>) -> Result<(), ProgramError> {
    match args.keys().find(|k| !shape::is_ident(k)) {
        Some(_) => Err(invalid(&at.key("args"), format!("must be {}", Ty::ArgMap.expected()))),
        None => Ok(()),
    }
}

fn want_var(v: &VarDecl, at: &At<'_>) -> Result<(), ProgramError> {
    want_str(&shape::VAR_DECL, "name", &v.name, at)
}

/// Validates an in-memory program with the same rules as [`Program::parse`], without
/// building a second tree. Literal and opaque contents are bounded on the encoded bytes.
#[derive(Default)]
struct Checker {
    count: usize,
    ids: BTreeSet<Uuid7>,
}

impl Checker {
    fn program(&mut self, p: &Program) -> Result<(), ProgramError> {
        if p.format_version != FORMAT_VERSION {
            return Err(ProgramError::FormatVersionUnsupported(p.format_version.to_string()));
        }
        let at_v = ROOT.key("variables");
        for (i, v) in p.variables.iter().enumerate() {
            want_var(v, &at_v.idx(i))?;
        }
        self.steps(&p.main, &ROOT.key("main"), 1)?;
        let at_s = ROOT.key("subroutines");
        for (i, sub) in p.subroutines.iter().enumerate() {
            let at = at_s.idx(i);
            want_str(&shape::SUBROUTINE, "name", &sub.name, &at)?;
            let at_p = at.key("params");
            for (j, v) in sub.params.iter().enumerate() {
                want_var(v, &at_p.idx(j))?;
            }
            self.steps(&sub.body, &at.key("body"), 1)?;
        }
        Ok(())
    }

    fn steps(&mut self, steps: &[Step], at: &At<'_>, depth: usize) -> Result<(), ProgramError> {
        if depth > MAX_STEP_NESTING {
            return Err(ProgramError::ProgramTooLarge(Bound::Depth));
        }
        for (i, st) in steps.iter().enumerate() {
            self.step(st, &at.idx(i), depth)?;
        }
        Ok(())
    }

    fn step(&mut self, st: &Step, at: &At<'_>, depth: usize) -> Result<(), ProgramError> {
        self.count += 1;
        if self.count > MAX_STEPS {
            return Err(ProgramError::ProgramTooLarge(Bound::Steps));
        }
        if !self.ids.insert(st.step_id) {
            return Err(ProgramError::DuplicateStepId(st.step_id));
        }
        match &st.kind {
            StepKind::Invoke { command, major, args, bind } => {
                let sh = &shape::STEP_INVOKE;
                want_str(sh, "command", command, at)?;
                want_u32(sh, "major", *major, at)?;
                want_args(args, at)?;
                if let Some(b) = bind {
                    want_str(sh, "bind", b, at)?;
                }
            }
            StepKind::Assign { var, .. } => want_str(&shape::STEP_ASSIGN, "var", var, at)?,
            StepKind::If { branches, else_body } => {
                if branches.is_empty() {
                    let ty = field_ty(&shape::STEP_IF, "branches");
                    return Err(invalid(&at.key("branches"), format!("must be {}", ty.expected())));
                }
                let at_b = at.key("branches");
                for (i, b) in branches.iter().enumerate() {
                    self.steps(&b.body, &at_b.idx(i).key("body"), depth + 1)?;
                }
                self.steps(else_body, &at.key("else_body"), depth + 1)?;
            }
            StepKind::Loop { mode, max_iterations, body } => {
                let sh = match mode {
                    LoopMode::Counted { var, .. } => {
                        want_str(&shape::STEP_LOOP_COUNTED, "var", var, at)?;
                        &shape::STEP_LOOP_COUNTED
                    }
                    LoopMode::While { .. } => &shape::STEP_LOOP_WHILE,
                };
                want_u32(sh, "max_iterations", *max_iterations, at)?;
                self.steps(body, &at.key("body"), depth + 1)?;
            }
            StepKind::Call { subroutine, args, bind } => {
                let sh = &shape::STEP_CALL;
                want_str(sh, "subroutine", subroutine, at)?;
                want_args(args, at)?;
                if let Some(b) = bind {
                    want_str(sh, "bind", b, at)?;
                }
            }
            StepKind::Label { name } => want_str(&shape::STEP_LABEL, "name", name, at)?,
            StepKind::Jump { label, .. } => want_str(&shape::STEP_JUMP, "label", label, at)?,
            StepKind::Return { .. } | StepKind::Operator { .. } | StepKind::Comment { .. } => {}
        }
        Ok(())
    }
}

// ------------------------------------------------------------------ encoding

/// A borrowed field value. The encoder writes straight from the program, so storing a
/// program never holds a second copy of its tree (REQ-AUTO.PROG.FMT-006, AC-6).
enum Val<'a> {
    Str(&'a str),
    Id(Uuid7),
    Num(u32),
    Json(&'a CValue),
    Obj(&'a Opaque),
    Objs(&'a [Opaque]),
    Vars(&'a [VarDecl]),
    Subs(&'a [Subroutine]),
    Steps(&'a [Step]),
    Branches(&'a [Branch]),
    Args(&'a BTreeMap<String, Arg>),
}

/// RFC 8785 §3.2.3: property names sort by their UTF-16 code units.
fn utf16_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}

struct Encoder {
    out: Vec<u8>,
}

impl Encoder {
    /// RFC 8785 §3.2.2.2 string serialisation.
    fn string(&mut self, s: &str) {
        self.out.push(b'"');
        for c in s.chars() {
            match c {
                '"' => self.out.extend_from_slice(b"\\\""),
                '\\' => self.out.extend_from_slice(b"\\\\"),
                '\u{8}' => self.out.extend_from_slice(b"\\b"),
                '\t' => self.out.extend_from_slice(b"\\t"),
                '\n' => self.out.extend_from_slice(b"\\n"),
                '\u{c}' => self.out.extend_from_slice(b"\\f"),
                '\r' => self.out.extend_from_slice(b"\\r"),
                c if (c as u32) < 0x20 => self.out.extend_from_slice(format!("\\u{:04x}", c as u32).as_bytes()),
                c => {
                    let mut buf = [0u8; 4];
                    self.out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
                }
            }
        }
        self.out.push(b'"');
    }

    fn json(&mut self, v: &CValue) -> Result<(), ProgramError> {
        let bytes = to_canonical_bytes(v).map_err(|e| ProgramError::InvalidJson(e.code()))?;
        self.out.extend_from_slice(&bytes);
        Ok(())
    }

    /// An object of fixed field names (all ASCII, so byte order is UTF-16 order).
    fn object(&mut self, mut fields: Vec<(&str, Val<'_>)>, depth: usize) -> Result<(), ProgramError> {
        fields.sort_by(|a, b| utf16_cmp(a.0, b.0));
        self.out.push(b'{');
        for (i, (k, v)) in fields.into_iter().enumerate() {
            if i > 0 {
                self.out.push(b',');
            }
            self.string(k);
            self.out.push(b':');
            self.val(v, depth)?;
        }
        self.out.push(b'}');
        Ok(())
    }

    fn opaque(&mut self, m: &Opaque) -> Result<(), ProgramError> {
        let mut keys: Vec<(&String, &CValue)> = m.iter().collect();
        keys.sort_by(|a, b| utf16_cmp(a.0, b.0));
        self.out.push(b'{');
        for (i, (k, v)) in keys.into_iter().enumerate() {
            if i > 0 {
                self.out.push(b',');
            }
            self.string(k);
            self.out.push(b':');
            self.json(v)?;
        }
        self.out.push(b'}');
        Ok(())
    }

    fn array<T>(&mut self, items: &[T], mut each: impl FnMut(&mut Self, &T) -> Result<(), ProgramError>) -> Result<(), ProgramError> {
        self.out.push(b'[');
        for (i, item) in items.iter().enumerate() {
            if i > 0 {
                self.out.push(b',');
            }
            each(self, item)?;
        }
        self.out.push(b']');
        Ok(())
    }

    fn val(&mut self, v: Val<'_>, depth: usize) -> Result<(), ProgramError> {
        match v {
            Val::Str(s) => self.string(s),
            Val::Id(id) => self.string(&id.to_string()),
            Val::Num(n) => self.out.extend_from_slice(n.to_string().as_bytes()),
            Val::Json(j) => self.json(j)?,
            Val::Obj(m) => self.opaque(m)?,
            Val::Objs(items) => self.array(items, |e, m| e.opaque(m))?,
            Val::Vars(items) => self.array(items, |e, v| {
                e.object(vec![("name", Val::Str(&v.name)), ("type", Val::Str(&v.ty))], depth)
            })?,
            Val::Subs(items) => self.array(items, |e, sub| {
                let mut f = vec![("name", Val::Str(&sub.name)), ("params", Val::Vars(&sub.params)), ("body", Val::Steps(&sub.body))];
                if let Some(r) = &sub.returns {
                    f.push(("returns", Val::Str(r)));
                }
                e.object(f, depth)
            })?,
            Val::Steps(steps) => {
                if depth >= MAX_STEP_NESTING {
                    return Err(ProgramError::ProgramTooLarge(Bound::Depth));
                }
                self.array(steps, |e, st| e.step(st, depth + 1))?
            }
            Val::Branches(items) => self.array(items, |e, b| {
                e.object(vec![("cond", Val::Str(&b.cond)), ("body", Val::Steps(&b.body))], depth)
            })?,
            Val::Args(args) => {
                let mut keys: Vec<(&String, &Arg)> = args.iter().collect();
                keys.sort_by(|a, b| utf16_cmp(a.0, b.0));
                self.out.push(b'{');
                for (i, (k, a)) in keys.into_iter().enumerate() {
                    if i > 0 {
                        self.out.push(b',');
                    }
                    self.string(k);
                    self.out.push(b':');
                    let f = match a {
                        Arg::Literal(l) => ("literal", Val::Json(l)),
                        Arg::Expr(x) => ("expr", Val::Str(x)),
                    };
                    self.object(vec![f], depth)?;
                }
                self.out.push(b'}');
            }
        }
        Ok(())
    }

    fn step(&mut self, st: &Step, depth: usize) -> Result<(), ProgramError> {
        let mut f = vec![("step_id", Val::Id(st.step_id)), ("kind", Val::Str(st.kind.as_str()))];
        match &st.kind {
            StepKind::Invoke { command, major, args, bind } => {
                f.extend([("command", Val::Str(command)), ("major", Val::Num(*major)), ("args", Val::Args(args))]);
                if let Some(b) = bind {
                    f.push(("bind", Val::Str(b)));
                }
            }
            StepKind::Assign { var, expr } => f.extend([("var", Val::Str(var)), ("expr", Val::Str(expr))]),
            StepKind::If { branches, else_body } => {
                f.extend([("branches", Val::Branches(branches)), ("else_body", Val::Steps(else_body))]);
            }
            StepKind::Loop { mode, max_iterations, body } => {
                match mode {
                    LoopMode::Counted { var, from, to, step } => f.extend([
                        ("mode", Val::Str("counted")),
                        ("var", Val::Str(var)),
                        ("from", Val::Str(from)),
                        ("to", Val::Str(to)),
                        ("step", Val::Str(step)),
                    ]),
                    LoopMode::While { cond } => f.extend([("mode", Val::Str("while")), ("cond", Val::Str(cond))]),
                }
                f.extend([("max_iterations", Val::Num(*max_iterations)), ("body", Val::Steps(body))]);
            }
            StepKind::Call { subroutine, args, bind } => {
                f.extend([("subroutine", Val::Str(subroutine)), ("args", Val::Args(args))]);
                if let Some(b) = bind {
                    f.push(("bind", Val::Str(b)));
                }
            }
            StepKind::Return { expr } => {
                if let Some(e) = expr {
                    f.push(("expr", Val::Str(e)));
                }
            }
            StepKind::Label { name } => f.push(("name", Val::Str(name))),
            StepKind::Jump { label, cond } => {
                f.push(("label", Val::Str(label)));
                if let Some(c) = cond {
                    f.push(("cond", Val::Str(c)));
                }
            }
            StepKind::Operator { descriptor } => f.push(("descriptor", Val::Obj(descriptor))),
            StepKind::Comment { text } => f.push(("text", Val::Str(text))),
        }
        self.object(f, depth)
    }
}

impl Program {
    fn encode_bytes(&self) -> Result<Vec<u8>, ProgramError> {
        let mut e = Encoder { out: Vec::new() };
        e.object(
            vec![
                ("format", Val::Str(FORMAT)),
                ("format_version", Val::Num(self.format_version)),
                ("program_id", Val::Id(self.program_id)),
                ("meta", Val::Obj(&self.meta)),
                ("params", Val::Objs(&self.params)),
                ("variables", Val::Vars(&self.variables)),
                ("subroutines", Val::Subs(&self.subroutines)),
                ("main", Val::Steps(&self.main)),
            ],
            0,
        )?;
        Ok(e.out)
    }
}

/// Every step kind, in declaration order.
pub const STEP_KINDS: [&str; 10] =
    ["invoke", "assign", "if", "loop", "call", "return", "label", "jump", "operator", "comment"];

/// Every field name a step of this kind may carry, from the field tables. Lets callers
/// (and tests) confirm that only `invoke` has a `command` (REQ-AUTO.PROG.FMT-004).
pub fn step_fields(kind: &str) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = shape::STEP_SHAPES
        .iter()
        .filter(|s| s.fields.iter().any(|f| f.name == "kind" && matches!(f.ty, Ty::Const(c) if c == kind)))
        .flat_map(|s| s.fields.iter().map(|f| f.name))
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}
