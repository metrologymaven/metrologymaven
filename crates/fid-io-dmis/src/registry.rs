//! Statement descriptors, the shape language and the descriptor registry (spec node
//! `IO.DMIS.PARSE.TREE`).
//!
//! A [`StatementDescriptor`] says which logical statements form one statement class: the
//! major word, the label rule, and a [`Shape`] for the comma-separated fields after the
//! slash. Grammar families (`IO.DMIS.PARSE.GDEF`, `IO.DMIS.PARSE.GEXEC`) and dialect profiles
//! ([`Dialect`]) register descriptors in a [`Registry`]; a duplicate class ID, or two
//! descriptors with the same major word, label rule and shape, is rejected when the registry
//! is built. The registry also generates the statement-class list `dmis-statement-classes.json`.
//!
//! Shapes are compiled into a small backtracking program when they are registered and matched
//! without recursion on the input: the matcher keeps an explicit stack and stops after
//! [`MAX_MATCH_STEPS`] steps per statement. Only a parenthesised-group terminal starts a
//! nested match, so the nesting depth follows the descriptor's shape (at most
//! [`MAX_SHAPE_DEPTH`]), never the input.

use crate::lex::{LexOptions, TokenKind};
use crate::tree::{Arg, Item};
use fid_canon::{to_canonical_bytes, CValue};
use std::collections::BTreeMap;
use std::fmt;

/// Most matcher steps for one statement, over all of its descriptors (10^4, spec Design).
pub const MAX_MATCH_STEPS: u32 = 10_000;
/// Deepest shape nesting a descriptor may use.
pub const MAX_SHAPE_DEPTH: usize = 16;
/// Most compiled instructions in one descriptor, nested groups included.
pub const MAX_PROGRAM_LEN: usize = 4096;

/// Schema name of the statement-class list.
pub const CLASSES_SCHEMA: &str = "fid.dmis-classes";
/// Schema version of the statement-class list.
pub const CLASSES_VERSION: u32 = 1;
/// File name of the statement-class list.
pub const CLASSES_FILE: &str = "dmis-statement-classes.json";

/// The label rule of a statement class.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LabelKind {
    /// The statement takes no `label =` prefix.
    None,
    /// The statement needs a label with this prefix (upper case), such as `F`.
    Required(Box<str>),
    /// The statement may have a label with this prefix.
    Optional(Box<str>),
}

impl LabelKind {
    /// The form used in `dmis-statement-classes.json`: `none`, `required:<P>` or `optional:<P>`.
    pub fn as_json(&self) -> String {
        match self {
            LabelKind::None => "none".to_string(),
            LabelKind::Required(p) => format!("required:{p}"),
            LabelKind::Optional(p) => format!("optional:{p}"),
        }
    }

    fn prefix(&self) -> Option<&str> {
        match self {
            LabelKind::None => None,
            LabelKind::Required(p) | LabelKind::Optional(p) => Some(p),
        }
    }
}

/// The shape of a statement's fields. Terminals match exactly one comma-separated field
/// that holds one item of the named kind; the combinators work on sequences of fields.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Shape {
    /// A field that is this word (compared in upper case), such as a minor word `POINT`.
    Keyword(Box<str>),
    /// A field that is any word.
    AnyWord,
    /// A field that is a number.
    Number,
    /// A field that is a quoted string.
    Str,
    /// A field that is a label reference, with this prefix if given.
    Label(Option<Box<str>>),
    /// A field that is the raw text of a `TEXT` statement.
    RawText,
    /// A field that is one parenthesised group whose fields match the inner shape.
    Group(Box<Shape>),
    /// Any one field, including an empty one or one of several items.
    AnyField,
    /// These shapes one after the other.
    Seq(Vec<Shape>),
    /// The first of these shapes that lets the whole statement match (at least one).
    Alt(Vec<Shape>),
    /// The shape or nothing.
    Opt(Box<Shape>),
    /// `item` at least `min` times and at most `max` times (`None`: no upper bound other than
    /// the statement's field count). `item` must not match zero fields.
    Repeat {
        /// The repeated shape.
        item: Box<Shape>,
        /// Fewest repetitions.
        min: u32,
        /// Most repetitions.
        max: Option<u32>,
    },
}

impl Shape {
    /// [`Shape::Keyword`] with `word` in upper case.
    pub fn kw(word: &str) -> Shape {
        Shape::Keyword(word.to_ascii_uppercase().into_boxed_str())
    }

    /// [`Shape::Seq`].
    pub fn seq(items: Vec<Shape>) -> Shape {
        Shape::Seq(items)
    }

    /// [`Shape::Alt`].
    pub fn alt(items: Vec<Shape>) -> Shape {
        Shape::Alt(items)
    }

    /// [`Shape::Opt`].
    pub fn opt(item: Shape) -> Shape {
        Shape::Opt(Box::new(item))
    }

    /// [`Shape::Repeat`].
    pub fn repeat(item: Shape, min: u32, max: Option<u32>) -> Shape {
        Shape::Repeat { item: Box::new(item), min, max }
    }

    /// [`Shape::Label`] with an upper-case prefix.
    pub fn label(prefix: &str) -> Shape {
        Shape::Label(Some(prefix.to_ascii_uppercase().into_boxed_str()))
    }

    /// [`Shape::Group`].
    pub fn group(inner: Shape) -> Shape {
        Shape::Group(Box::new(inner))
    }
}

/// One statement class.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StatementDescriptor {
    /// Class ID, `[a-z][a-z0-9_]*`, such as `feat_point`.
    pub class: Box<str>,
    /// Major word, `[A-Z][A-Z0-9_]*` (upper case), such as `FEAT`.
    pub major: Box<str>,
    /// Label rule.
    pub label_kind: LabelKind,
    /// Shape of the fields after the slash. A statement without a slash has zero fields.
    pub shape: Shape,
    /// Spec node of the family that registers it, such as `IO.DMIS.PARSE.GDEF`.
    pub family: Box<str>,
    /// Phase tag, such as `P1a`.
    pub phase: Box<str>,
}

impl StatementDescriptor {
    /// A descriptor; `major` and label prefixes are taken in upper case.
    pub fn new(class: &str, major: &str, label_kind: LabelKind, shape: Shape, family: &str, phase: &str) -> Self {
        let label_kind = match label_kind {
            LabelKind::None => LabelKind::None,
            LabelKind::Required(p) => LabelKind::Required(p.to_ascii_uppercase().into_boxed_str()),
            LabelKind::Optional(p) => LabelKind::Optional(p.to_ascii_uppercase().into_boxed_str()),
        };
        StatementDescriptor {
            class: class.into(),
            major: major.to_ascii_uppercase().into_boxed_str(),
            label_kind,
            shape,
            family: family.into(),
            phase: phase.into(),
        }
    }
}

/// A dialect profile's contribution (data supplied by `IO.DMIS.DIAL`): extra descriptors and
/// lexer options.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Dialect {
    /// Dialect name.
    pub name: Box<str>,
    /// Descriptors added on top of the base registry.
    pub descriptors: Vec<StatementDescriptor>,
    /// Lexer options of the dialect.
    pub lex: LexOptions,
}

/// Why a registry could not be built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryError {
    /// Two descriptors use the same class ID.
    DuplicateClass {
        /// The class ID.
        class: Box<str>,
    },
    /// Two descriptors have the same major word, label rule and shape.
    DuplicateDescriptor {
        /// The class registered second.
        class: Box<str>,
        /// The class registered first.
        existing: Box<str>,
    },
    /// A descriptor breaks a rule of the shape language or of the naming rules.
    InvalidDescriptor {
        /// The class ID as given.
        class: Box<str>,
        /// Which rule.
        reason: &'static str,
    },
    /// The class list could not be encoded.
    Canonical(String),
}

impl RegistryError {
    /// Stable error code.
    pub fn code(&self) -> &'static str {
        match self {
            RegistryError::DuplicateClass { .. } => "duplicate_class",
            RegistryError::DuplicateDescriptor { .. } => "duplicate_descriptor",
            RegistryError::InvalidDescriptor { .. } => "invalid_descriptor",
            RegistryError::Canonical(_) => "canonical",
        }
    }
}

impl fmt::Display for RegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RegistryError::DuplicateClass { class } => write!(f, "duplicate_class: {class}"),
            RegistryError::DuplicateDescriptor { class, existing } => {
                write!(f, "duplicate_descriptor: {class} repeats {existing}")
            }
            RegistryError::InvalidDescriptor { class, reason } => write!(f, "invalid_descriptor: {class}: {reason}"),
            RegistryError::Canonical(e) => write!(f, "canonical: {e}"),
        }
    }
}

impl std::error::Error for RegistryError {}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Term {
    Keyword(Box<str>),
    AnyWord,
    Number,
    Str,
    Label(Option<Box<str>>),
    RawText,
    Group(Program),
    AnyField,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Inst {
    /// Match one field and go on.
    Term(Term),
    /// Try the first target; on failure come back and try the second.
    Split(usize, usize),
    Jmp(usize),
    /// Succeed if every field is used.
    Done,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Program(Vec<Inst>);

#[derive(Debug, Clone)]
struct Compiled {
    desc: StatementDescriptor,
    program: Program,
}

/// The descriptor registry: descriptors by major word, in registration order.
#[derive(Debug, Clone, Default)]
pub struct Registry {
    by_major: BTreeMap<Box<str>, Vec<Compiled>>,
    classes: BTreeMap<Box<str>, Box<str>>,
}

/// The descriptors registered by the built-in grammar families. The families
/// `IO.DMIS.PARSE.GDEF` and `IO.DMIS.PARSE.GEXEC` add theirs here; neither is built yet, so
/// the list is empty and every statement parses as `Unknown`.
pub fn builtin_descriptors() -> Vec<StatementDescriptor> {
    Vec::new()
}

fn valid_class(s: &str) -> bool {
    let mut b = s.bytes();
    matches!(b.next(), Some(b'a'..=b'z')) && b.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
}

fn valid_major(s: &str) -> bool {
    let mut b = s.bytes();
    matches!(b.next(), Some(b'A'..=b'Z')) && b.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == b'_')
}

fn valid_prefix(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|c| c.is_ascii_uppercase())
}

fn valid_tag(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|c| c.is_ascii_graphic())
}

struct Compiler<'a> {
    class: &'a str,
    total: usize,
}

impl Compiler<'_> {
    fn invalid(&self, reason: &'static str) -> RegistryError {
        RegistryError::InvalidDescriptor { class: self.class.into(), reason }
    }

    fn push(&mut self, out: &mut Vec<Inst>, inst: Inst) -> Result<(), RegistryError> {
        self.total += 1;
        if self.total > MAX_PROGRAM_LEN {
            return Err(self.invalid("program_too_long"));
        }
        out.push(inst);
        Ok(())
    }

    /// Appends `shape` to `out`; returns true if it can match zero fields. Recursion follows
    /// the descriptor's shape and stops at [`MAX_SHAPE_DEPTH`].
    fn compile(&mut self, shape: &Shape, out: &mut Vec<Inst>, depth: usize) -> Result<bool, RegistryError> {
        if depth > MAX_SHAPE_DEPTH {
            return Err(self.invalid("shape_too_deep"));
        }
        let term = |t| Some(Inst::Term(t));
        let simple = match shape {
            Shape::Keyword(w) => {
                if !valid_major(w) {
                    return Err(self.invalid("bad_keyword"));
                }
                term(Term::Keyword(w.clone()))
            }
            Shape::AnyWord => term(Term::AnyWord),
            Shape::Number => term(Term::Number),
            Shape::Str => term(Term::Str),
            Shape::Label(p) => {
                if p.as_deref().is_some_and(|p| !valid_prefix(p)) {
                    return Err(self.invalid("bad_label_prefix"));
                }
                term(Term::Label(p.clone()))
            }
            Shape::RawText => term(Term::RawText),
            Shape::AnyField => term(Term::AnyField),
            _ => None,
        };
        if let Some(inst) = simple {
            self.push(out, inst)?;
            return Ok(false);
        }
        match shape {
            Shape::Group(inner) => {
                let mut sub = Vec::new();
                self.compile(inner, &mut sub, depth + 1)?;
                self.push(&mut sub, Inst::Done)?;
                self.push(out, Inst::Term(Term::Group(Program(sub))))?;
                Ok(false)
            }
            Shape::Seq(items) => {
                let mut nullable = true;
                for s in items {
                    nullable &= self.compile(s, out, depth + 1)?;
                }
                Ok(nullable)
            }
            Shape::Alt(items) => {
                if items.is_empty() {
                    return Err(self.invalid("empty_alternative"));
                }
                let mut nullable = false;
                let mut jumps = Vec::new();
                for (k, s) in items.iter().enumerate() {
                    let last = k + 1 == items.len();
                    let split = out.len();
                    if !last {
                        self.push(out, Inst::Split(split + 1, 0))?;
                    }
                    nullable |= self.compile(s, out, depth + 1)?;
                    if !last {
                        jumps.push(out.len());
                        self.push(out, Inst::Jmp(0))?;
                        let next = out.len();
                        if let Some(Inst::Split(_, b)) = out.get_mut(split) {
                            *b = next;
                        }
                    }
                }
                let end = out.len();
                for j in jumps {
                    if let Some(Inst::Jmp(t)) = out.get_mut(j) {
                        *t = end;
                    }
                }
                Ok(nullable)
            }
            Shape::Opt(item) => {
                let split = out.len();
                self.push(out, Inst::Split(split + 1, 0))?;
                self.compile(item, out, depth + 1)?;
                let end = out.len();
                if let Some(Inst::Split(_, b)) = out.get_mut(split) {
                    *b = end;
                }
                Ok(true)
            }
            Shape::Repeat { item, min, max } => {
                if max.is_some_and(|m| m < *min || m == 0) {
                    return Err(self.invalid("bad_repeat_bounds"));
                }
                for _ in 0..*min {
                    if self.compile(item, out, depth + 1)? {
                        return Err(self.invalid("nullable_repeat"));
                    }
                }
                match max {
                    Some(m) => {
                        let mut splits = Vec::new();
                        for _ in *min..*m {
                            let at = out.len();
                            splits.push(at);
                            self.push(out, Inst::Split(at + 1, 0))?;
                            if self.compile(item, out, depth + 1)? {
                                return Err(self.invalid("nullable_repeat"));
                            }
                        }
                        let end = out.len();
                        for s in splits {
                            if let Some(Inst::Split(_, b)) = out.get_mut(s) {
                                *b = end;
                            }
                        }
                    }
                    None => {
                        let top = out.len();
                        self.push(out, Inst::Split(top + 1, 0))?;
                        if self.compile(item, out, depth + 1)? {
                            return Err(self.invalid("nullable_repeat"));
                        }
                        self.push(out, Inst::Jmp(top))?;
                        let end = out.len();
                        if let Some(Inst::Split(_, b)) = out.get_mut(top) {
                            *b = end;
                        }
                    }
                }
                Ok(*min == 0)
            }
            _ => Err(self.invalid("unknown_shape")),
        }
    }
}

fn compile(desc: &StatementDescriptor) -> Result<Program, RegistryError> {
    let class = &*desc.class;
    let invalid = |reason| RegistryError::InvalidDescriptor { class: class.into(), reason };
    if !valid_class(class) {
        return Err(invalid("bad_class_id"));
    }
    if !valid_major(&desc.major) {
        return Err(invalid("bad_major_word"));
    }
    if desc.label_kind.prefix().is_some_and(|p| !valid_prefix(p)) {
        return Err(invalid("bad_label_prefix"));
    }
    if !valid_tag(&desc.family) || !valid_tag(&desc.phase) {
        return Err(invalid("bad_family_or_phase"));
    }
    let mut c = Compiler { class, total: 0 };
    let mut out = Vec::new();
    c.compile(&desc.shape, &mut out, 0)?;
    c.push(&mut out, Inst::Done)?;
    Ok(Program(out))
}

/// The matcher ran past [`MAX_MATCH_STEPS`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StepsExceeded;

/// Outcome of matching one descriptor against a statement's fields.
pub(crate) struct Attempt<'r> {
    pub(crate) desc: &'r StatementDescriptor,
    /// `None` when the label rule failed; else whether the shape matched and the farthest
    /// field index any path reached before failing.
    pub(crate) result: Option<(bool, usize)>,
}

fn term_matches(term: &Term, arg: &Arg, steps: &mut u32) -> Result<bool, StepsExceeded> {
    if let Term::AnyField = term {
        return Ok(true);
    }
    let Some(item) = arg.single() else { return Ok(false) };
    let token = match item {
        Item::Group(g) => {
            return match term {
                Term::Group(p) => Ok(run(p, &g.args, steps)?.0),
                _ => Ok(false),
            }
        }
        Item::Atom(t) => t,
    };
    Ok(match (term, &token.kind) {
        (Term::Keyword(w), TokenKind::Word { upper }) => upper == w,
        (Term::AnyWord, TokenKind::Word { .. }) => true,
        (Term::Number, TokenKind::Number) => true,
        (Term::Str, TokenKind::Str { .. }) => true,
        (Term::Label(None), TokenKind::Label { .. }) => true,
        (Term::Label(Some(p)), TokenKind::Label { prefix, .. }) => prefix == p,
        (Term::RawText, TokenKind::RawText) => true,
        _ => false,
    })
}

/// Runs `program` over `args`: (matched, farthest failing field index). No recursion on the
/// input: alternatives wait on an explicit stack.
fn run(program: &Program, args: &[Arg], steps: &mut u32) -> Result<(bool, usize), StepsExceeded> {
    let mut stack: Vec<(usize, usize)> = vec![(0, 0)];
    let mut farthest = 0usize;
    while let Some((mut pc, mut fi)) = stack.pop() {
        loop {
            *steps = steps.saturating_add(1);
            if *steps > MAX_MATCH_STEPS {
                return Err(StepsExceeded);
            }
            match program.0.get(pc) {
                None => break,
                Some(Inst::Done) => {
                    if fi == args.len() {
                        return Ok((true, fi));
                    }
                    farthest = farthest.max(fi);
                    break;
                }
                Some(Inst::Jmp(t)) => pc = *t,
                Some(Inst::Split(a, b)) => {
                    stack.push((*b, fi));
                    pc = *a;
                }
                Some(Inst::Term(t)) => match args.get(fi) {
                    Some(arg) if term_matches(t, arg, steps)? => {
                        fi += 1;
                        pc += 1;
                    }
                    _ => {
                        farthest = farthest.max(fi);
                        break;
                    }
                },
            }
        }
    }
    Ok((false, farthest))
}

impl Registry {
    /// An empty registry.
    pub fn new() -> Self {
        Registry::default()
    }

    /// A registry holding `descriptors`, in order.
    pub fn from_descriptors<I: IntoIterator<Item = StatementDescriptor>>(
        descriptors: I,
    ) -> Result<Self, RegistryError> {
        let mut r = Registry::new();
        for d in descriptors {
            r.add(d)?;
        }
        Ok(r)
    }

    /// The registry of the built-in grammar families ([`builtin_descriptors`]).
    pub fn builtin() -> Result<Self, RegistryError> {
        Registry::from_descriptors(builtin_descriptors())
    }

    /// Registers one descriptor; rejects duplicates and invalid shapes.
    pub fn add(&mut self, desc: StatementDescriptor) -> Result<(), RegistryError> {
        let program = compile(&desc)?;
        if self.classes.contains_key(&desc.class) {
            return Err(RegistryError::DuplicateClass { class: desc.class });
        }
        let list = self.by_major.entry(desc.major.clone()).or_default();
        if let Some(other) = list.iter().find(|c| c.desc.label_kind == desc.label_kind && c.desc.shape == desc.shape) {
            return Err(RegistryError::DuplicateDescriptor { class: desc.class, existing: other.desc.class.clone() });
        }
        self.classes.insert(desc.class.clone(), desc.major.clone());
        list.push(Compiled { desc, program });
        Ok(())
    }

    /// This registry plus the dialect's descriptors.
    pub fn with_dialect(&self, dialect: &Dialect) -> Result<Self, RegistryError> {
        let mut r = self.clone();
        for d in &dialect.descriptors {
            r.add(d.clone())?;
        }
        Ok(r)
    }

    /// True if `major` (upper case) has a descriptor.
    pub fn knows(&self, major: &str) -> bool {
        self.by_major.contains_key(major)
    }

    /// Number of registered classes.
    pub fn len(&self) -> usize {
        self.classes.len()
    }

    /// True if no class is registered.
    pub fn is_empty(&self) -> bool {
        self.classes.is_empty()
    }

    /// Every descriptor, sorted by class ID.
    pub fn descriptors(&self) -> Vec<&StatementDescriptor> {
        let mut all: Vec<&StatementDescriptor> = self.by_major.values().flatten().map(|c| &c.desc).collect();
        all.sort_by(|a, b| a.class.cmp(&b.class));
        all
    }

    /// The descriptors of `major` in registration order.
    pub(crate) fn for_major(&self, major: &str) -> Vec<&StatementDescriptor> {
        self.by_major.get(major).map(|v| v.iter().map(|c| &c.desc).collect()).unwrap_or_default()
    }

    /// Tries every descriptor of `major` in registration order and stops at the first match.
    pub(crate) fn attempt<'r>(
        &'r self,
        major: &str,
        label_prefix: Option<&str>,
        args: &[Arg],
    ) -> Result<Vec<Attempt<'r>>, StepsExceeded> {
        let mut steps = 0u32;
        let mut out = Vec::new();
        for c in self.by_major.get(major).map(Vec::as_slice).unwrap_or(&[]) {
            let label_ok = match (&c.desc.label_kind, label_prefix) {
                (LabelKind::None, None) => true,
                (LabelKind::Optional(_), None) => true,
                (LabelKind::Required(p) | LabelKind::Optional(p), Some(q)) => **p == *q,
                _ => false,
            };
            let result = if label_ok { Some(run(&c.program, args, &mut steps)?) } else { None };
            let done = matches!(result, Some((true, _)));
            out.push(Attempt { desc: &c.desc, result });
            if done {
                break;
            }
        }
        Ok(out)
    }

    /// `dmis-statement-classes.json` (schema `fid.dmis-classes` v1, RFC 8785 canonical JSON):
    /// one entry per class, sorted by class ID.
    pub fn classes_json(&self) -> Result<Vec<u8>, RegistryError> {
        let s = |v: &str| CValue::Str(v.to_string());
        let classes = self
            .descriptors()
            .into_iter()
            .map(|d| {
                let mut o = BTreeMap::new();
                o.insert("class".to_string(), s(&d.class));
                o.insert("major".to_string(), s(&d.major));
                o.insert("label_kind".to_string(), s(&d.label_kind.as_json()));
                o.insert("family".to_string(), s(&d.family));
                o.insert("phase".to_string(), s(&d.phase));
                CValue::Obj(o)
            })
            .collect();
        let mut root = BTreeMap::new();
        root.insert("schema".to_string(), s(CLASSES_SCHEMA));
        root.insert("version".to_string(), CValue::Num(f64::from(CLASSES_VERSION)));
        root.insert("classes".to_string(), CValue::Arr(classes));
        to_canonical_bytes(&CValue::Obj(root)).map_err(|e| RegistryError::Canonical(e.to_string()))
    }
}
