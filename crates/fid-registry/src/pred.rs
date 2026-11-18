//! FPL, the precondition language, and its bounded evaluator. Spec node `CORE.REG.DEF.PRED`.
//!
//! FPL is a small, non-Turing-complete expression language with CEL-like syntax, written
//! in-house (it is not a conforming CEL implementation). It offers literals, field access,
//! comparison, `&&` `||` `!`, `?:`, `in`, `size`, `has`, and the bounded list functions
//! `all(x in list, body)` and `exists(x in list, body)`. There are no loops, recursion,
//! assignment, user functions, I/O, clock or randomness (REQ-001).
//!
//! Limits: an expression is at most [`MAX_SRC_BYTES`] bytes, and the AST and the parser nest
//! at most [`MAX_DEPTH`] levels. A flat chain `a && b && c` (or `||`) is one n-ary node, so
//! it does not count as nesting. Every evaluation stops at its declared step bound (at most
//! [`MAX_BOUND`]). The evaluator walks the tree with an explicit work stack, counts one step
//! per AST node visited and one per list element visited by `all`/`exists`, and checks the
//! counter before each visit (REQ-003, REQ-004). `&&`, `||`, `?:`, `all` and `exists`
//! short-circuit, left to right. It reads state only through [`PredContext`] (REQ-006).
//!
//! Cost model: a context path read (`params.a.b`), `in` and `size` each cost one step, but
//! their work is linear in the size of the value read or searched (O(n) within one step).
//! Values are bounded by what the [`PredContext`] supplies.
//!
//! Numbers: `-0.0` is normalised to `+0.0` when a value enters evaluation (float literals and
//! context reads); otherwise floats compare under IEEE 754 total order (REQ-007).
//!
//! The step bound and the message key are declared next to the expression, in
//! [`crate::def::Precondition`].

use crate::def::{CommandDef, Precondition};
use fid_canon::CValue;
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fmt;

/// Largest step bound a precondition may declare (REQ-003).
pub const MAX_BOUND: u32 = 10_000;
/// Nesting limit of the parser and of the AST.
pub const MAX_DEPTH: usize = 64;
/// Largest precondition declaration, in bytes.
pub const MAX_SRC_BYTES: usize = 4096;
/// Nesting limit when reading a parameter schema or a type name.
const MAX_SCHEMA_DEPTH: usize = 16;

/// FPL static types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    Bool,
    Int,
    Float,
    String,
    EntityRef,
    List(Box<Type>),
    Map,
}

impl Type {
    /// Parses `bool`, `int`, `float`, `string`, `ref`, `map` or `list<T>`.
    pub fn from_name(name: &str) -> Option<Type> {
        let mut s = name.trim();
        let mut lists = 0usize;
        while let Some(inner) = s.strip_prefix("list<").and_then(|r| r.strip_suffix('>')) {
            lists += 1;
            if lists > MAX_SCHEMA_DEPTH {
                return None;
            }
            s = inner.trim();
        }
        let mut t = match s {
            "bool" => Type::Bool,
            "int" => Type::Int,
            "float" => Type::Float,
            "string" => Type::String,
            "ref" => Type::EntityRef,
            "map" => Type::Map,
            _ => return None,
        };
        for _ in 0..lists {
            t = Type::List(Box::new(t));
        }
        Some(t)
    }

    fn numeric(&self) -> bool {
        matches!(self, Type::Int | Type::Float)
    }

    fn scalar(&self) -> bool {
        matches!(self, Type::Bool | Type::Int | Type::Float | Type::String | Type::EntityRef)
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Type::Bool => f.write_str("bool"),
            Type::Int => f.write_str("int"),
            Type::Float => f.write_str("float"),
            Type::String => f.write_str("string"),
            Type::EntityRef => f.write_str("ref"),
            Type::List(t) => write!(f, "list<{t}>"),
            Type::Map => f.write_str("map"),
        }
    }
}

/// Runtime values, as supplied by a [`PredContext`].
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    /// An entity reference (opaque ID); compares only with other references.
    Ref(String),
    List(Vec<Value>),
    Map(BTreeMap<String, Value>),
}

impl Value {
    fn kind(&self) -> &'static str {
        match self {
            Value::Bool(_) => "bool",
            Value::Int(_) => "int",
            Value::Float(_) => "float",
            Value::Str(_) => "string",
            Value::Ref(_) => "ref",
            Value::List(_) => "list",
            Value::Map(_) => "map",
        }
    }
}

/// A dotted path into the context, root first, e.g. `params.ids` or `doc.active.kind`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct FieldPath(pub Vec<String>);

impl fmt::Display for FieldPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0.join("."))
    }
}

/// The only way the evaluator reads state (REQ-006). `CORE.BUS.PIPE` implements it; the
/// `params` root holds the command's parameters.
pub trait PredContext {
    /// Returns the value at `path`, or [`PredError::MissingField`] if it is absent.
    fn get(&self, path: &FieldPath) -> Result<Value, PredError>;
}

/// Evaluation errors with stable codes (REQ-005, REQ-007).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PredError {
    /// The step bound was reached; `steps` equals the bound.
    Budget { steps: u32 },
    /// A NaN took part in a comparison; `pos` is the operator's byte offset.
    NanOperand { pos: usize },
    /// A field was absent.
    MissingField { path: String },
    /// A value had the wrong runtime type for its operation.
    TypeError { pos: usize, detail: String },
}

impl PredError {
    /// Stable code.
    pub fn code(&self) -> &'static str {
        match self {
            PredError::Budget { .. } => "precondition_budget",
            PredError::NanOperand { .. } => "nan_operand",
            PredError::MissingField { .. } => "missing_field",
            PredError::TypeError { .. } => "type_error",
        }
    }
}

impl fmt::Display for PredError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PredError::Budget { steps } => write!(f, "precondition_budget: stopped after {steps} steps"),
            PredError::NanOperand { pos } => write!(f, "nan_operand at byte {pos}"),
            PredError::MissingField { path } => write!(f, "missing_field: {path}"),
            PredError::TypeError { pos, detail } => write!(f, "type_error at byte {pos}: {detail}"),
        }
    }
}

impl std::error::Error for PredError {}

/// A parse or type error (`fpl_syntax` or `fpl_type`) at a byte offset of the declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub code: &'static str,
    pub pos: usize,
    pub message: String,
}

impl ParseError {
    fn syntax(pos: usize, message: impl Into<String>) -> Self {
        ParseError { code: "fpl_syntax", pos, message: message.into() }
    }
    fn ty(pos: usize, message: impl Into<String>) -> Self {
        ParseError { code: "fpl_type", pos, message: message.into() }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at byte {}: {}", self.code, self.pos, self.message)
    }
}

impl std::error::Error for ParseError {}

/// Comparison operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmpOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

/// One AST node: its kind, the byte offset of its token, and its nesting depth (leaf = 1).
#[derive(Debug, Clone, PartialEq)]
pub struct Expr {
    pub kind: ExprKind,
    pub pos: usize,
    pub depth: usize,
}

/// AST node kinds.
#[derive(Debug, Clone, PartialEq)]
pub enum ExprKind {
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    List(Vec<Expr>),
    /// A context read, one node however many segments.
    Path(FieldPath),
    /// A variable bound by `all` or `exists`.
    Var(String),
    /// Field access on a map value.
    Field(Box<Expr>, String),
    Not(Box<Expr>),
    /// A flat `&&` chain of two or more operands.
    And(Vec<Expr>),
    /// A flat `||` chain of two or more operands.
    Or(Vec<Expr>),
    Cmp(CmpOp, Box<Expr>, Box<Expr>),
    In(Box<Expr>, Box<Expr>),
    Cond(Box<Expr>, Box<Expr>, Box<Expr>),
    Size(Box<Expr>),
    Has(Box<Expr>),
    /// `all` (`is_all`) or `exists` over a list.
    Quant {
        is_all: bool,
        var: String,
        list: Box<Expr>,
        body: Box<Expr>,
    },
}

fn node(kind: ExprKind, pos: usize) -> Result<Expr, ParseError> {
    let child = match &kind {
        ExprKind::List(items) | ExprKind::And(items) | ExprKind::Or(items) => {
            items.iter().map(|e| e.depth).max().unwrap_or(0)
        }
        ExprKind::Field(a, _) | ExprKind::Not(a) | ExprKind::Size(a) | ExprKind::Has(a) => a.depth,
        ExprKind::Cmp(_, a, b) | ExprKind::In(a, b) => a.depth.max(b.depth),
        ExprKind::Quant { list, body, .. } => list.depth.max(body.depth),
        ExprKind::Cond(a, b, c) => a.depth.max(b.depth).max(c.depth),
        _ => 0,
    };
    let depth = child + 1;
    if depth > MAX_DEPTH {
        return Err(ParseError::syntax(pos, format!("expression nests deeper than {MAX_DEPTH}")));
    }
    Ok(Expr { kind, pos, depth })
}

// ---------------------------------------------------------------- lexer

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Ident(String),
    Int(i64),
    Float(f64),
    Str(String),
    LParen,
    RParen,
    LBrack,
    RBrack,
    Comma,
    Dot,
    Bang,
    AndAnd,
    OrOr,
    Cmp(CmpOp),
    Question,
    Colon,
    Eof,
}

/// Words FPL refuses where an identifier starts an expression or names a variable: loops,
/// definitions, assignment and other statement forms. After `.` they are ordinary field names.
const FORBIDDEN: &[&str] = &[
    "for", "while", "loop", "do", "foreach", "repeat", "until", "break", "continue", "goto", "def", "fn", "function",
    "lambda", "let", "var", "const", "return", "yield", "import", "load", "null", "nil", "now", "rand", "random",
];

fn lex(src: &str) -> Result<Vec<(Tok, usize)>, ParseError> {
    let b = src.as_bytes();
    let mut i = 0;
    let mut out = Vec::new();
    while i < b.len() {
        let c = b[i];
        let p = i;
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        let next = b.get(i + 1).copied();
        let two = |d: u8| next == Some(d);
        let tok = match c {
            b'(' => Tok::LParen,
            b')' => Tok::RParen,
            b'[' => Tok::LBrack,
            b']' => Tok::RBrack,
            b',' => Tok::Comma,
            b'.' => Tok::Dot,
            b'?' => Tok::Question,
            b':' => Tok::Colon,
            b'!' if two(b'=') => Tok::Cmp(CmpOp::Ne),
            b'!' => Tok::Bang,
            b'&' if two(b'&') => Tok::AndAnd,
            b'|' if two(b'|') => Tok::OrOr,
            b'=' if two(b'=') => Tok::Cmp(CmpOp::Eq),
            b'=' => return Err(ParseError::syntax(p, "assignment is not supported")),
            b'<' if two(b'=') => Tok::Cmp(CmpOp::Le),
            b'<' => Tok::Cmp(CmpOp::Lt),
            b'>' if two(b'=') => Tok::Cmp(CmpOp::Ge),
            b'>' => Tok::Cmp(CmpOp::Gt),
            b'-' if b.get(i + 1).is_some_and(u8::is_ascii_digit) => {
                let (t, end) = number(src, i)?;
                out.push((t, p));
                i = end;
                continue;
            }
            b'0'..=b'9' => {
                let (t, end) = number(src, i)?;
                out.push((t, p));
                i = end;
                continue;
            }
            b'"' | b'\'' => {
                let (s, end) = string(src, i)?;
                out.push((Tok::Str(s), p));
                i = end;
                continue;
            }
            b'a'..=b'z' | b'A'..=b'Z' | b'_' => {
                let mut end = i;
                while end < b.len() && (b[end].is_ascii_alphanumeric() || b[end] == b'_') {
                    end += 1;
                }
                out.push((Tok::Ident(src[i..end].to_string()), p));
                i = end;
                continue;
            }
            b'+' | b'-' | b'*' | b'/' | b'%' => return Err(ParseError::syntax(p, "arithmetic is not supported")),
            b';' | b'{' | b'}' => return Err(ParseError::syntax(p, "statements and blocks are not supported")),
            _ => return Err(ParseError::syntax(p, "unexpected character")),
        };
        i += match &tok {
            Tok::AndAnd | Tok::OrOr | Tok::Cmp(CmpOp::Eq | CmpOp::Ne | CmpOp::Le | CmpOp::Ge) => 2,
            _ => 1,
        };
        out.push((tok, p));
    }
    out.push((Tok::Eof, b.len()));
    Ok(out)
}

fn number(src: &str, start: usize) -> Result<(Tok, usize), ParseError> {
    let b = src.as_bytes();
    let mut i = start;
    if b[i] == b'-' {
        i += 1;
    }
    let digits = |mut j: usize| {
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        j
    };
    i = digits(i);
    let mut float = false;
    if i < b.len() && b[i] == b'.' && b.get(i + 1).is_some_and(u8::is_ascii_digit) {
        float = true;
        i = digits(i + 1);
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        let mut j = i + 1;
        if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
            j += 1;
        }
        if !b.get(j).is_some_and(u8::is_ascii_digit) {
            return Err(ParseError::syntax(start, "malformed number"));
        }
        float = true;
        i = digits(j);
    }
    let text = &src[start..i];
    if float {
        match text.parse::<f64>() {
            Ok(f) if f.is_finite() => Ok((Tok::Float(positive_zero(f)), i)),
            _ => Err(ParseError::syntax(start, "number out of range")),
        }
    } else {
        match text.parse::<i64>() {
            Ok(n) => Ok((Tok::Int(n), i)),
            Err(_) => Err(ParseError::syntax(start, "integer out of range")),
        }
    }
}

fn string(src: &str, start: usize) -> Result<(String, usize), ParseError> {
    let quote = if src.as_bytes().get(start) == Some(&b'\'') { '\'' } else { '"' };
    let mut out = String::new();
    let mut escape = false;
    for (k, c) in src[start + 1..].char_indices() {
        let at = start + 1 + k;
        if escape {
            escape = false;
            out.push(match c {
                'n' => '\n',
                't' => '\t',
                'r' => '\r',
                '\\' | '"' | '\'' => c,
                _ => return Err(ParseError::syntax(at, "unknown escape")),
            });
        } else if c == '\\' {
            escape = true;
        } else if c == quote {
            return Ok((out, at + 1));
        } else {
            out.push(c);
        }
    }
    Err(ParseError::syntax(start, "unterminated string"))
}

// ---------------------------------------------------------------- parser

struct Parser {
    toks: Vec<(Tok, usize)>,
    i: usize,
    level: usize,
    vars: Vec<String>,
}

impl Parser {
    fn peek(&self) -> &Tok {
        self.toks.get(self.i).map_or(&Tok::Eof, |t| &t.0)
    }
    fn pos(&self) -> usize {
        self.toks.get(self.i).map_or(0, |t| t.1)
    }
    fn bump(&mut self) -> (Tok, usize) {
        let t = self.toks.get(self.i).cloned().unwrap_or((Tok::Eof, 0));
        if self.i < self.toks.len() {
            self.i += 1;
        }
        t
    }
    fn expect(&mut self, want: &Tok, what: &str) -> Result<usize, ParseError> {
        if self.peek() == want {
            Ok(self.bump().1)
        } else {
            Err(ParseError::syntax(self.pos(), format!("expected {what}")))
        }
    }
    fn enter(&mut self) -> Result<(), ParseError> {
        self.level += 1;
        if self.level > MAX_DEPTH {
            return Err(ParseError::syntax(self.pos(), format!("expression nests deeper than {MAX_DEPTH}")));
        }
        Ok(())
    }

    fn expr(&mut self) -> Result<Expr, ParseError> {
        self.enter()?;
        let cond = self.or()?;
        let out = if *self.peek() == Tok::Question {
            let q = self.bump().1;
            let a = self.expr()?;
            self.expect(&Tok::Colon, "`:`")?;
            let b = self.expr()?;
            node(ExprKind::Cond(Box::new(cond), Box::new(a), Box::new(b)), q)?
        } else {
            cond
        };
        self.level -= 1;
        Ok(out)
    }

    fn or(&mut self) -> Result<Expr, ParseError> {
        let first = self.and()?;
        if *self.peek() != Tok::OrOr {
            return Ok(first);
        }
        let p = self.pos();
        let mut items = vec![first];
        while *self.peek() == Tok::OrOr {
            self.bump();
            items.push(self.and()?);
        }
        node(ExprKind::Or(items), p)
    }

    fn and(&mut self) -> Result<Expr, ParseError> {
        let first = self.rel()?;
        if *self.peek() != Tok::AndAnd {
            return Ok(first);
        }
        let p = self.pos();
        let mut items = vec![first];
        while *self.peek() == Tok::AndAnd {
            self.bump();
            items.push(self.rel()?);
        }
        node(ExprKind::And(items), p)
    }

    fn rel_op(&self) -> Option<Option<CmpOp>> {
        match self.peek() {
            Tok::Cmp(op) => Some(Some(*op)),
            Tok::Ident(w) if w == "in" => Some(None),
            _ => None,
        }
    }

    fn rel(&mut self) -> Result<Expr, ParseError> {
        let lhs = self.unary()?;
        let Some(op) = self.rel_op() else { return Ok(lhs) };
        let p = self.bump().1;
        let rhs = self.unary()?;
        if self.rel_op().is_some() {
            return Err(ParseError::syntax(self.pos(), "comparisons do not chain; use && or parentheses"));
        }
        let kind = match op {
            Some(op) => ExprKind::Cmp(op, Box::new(lhs), Box::new(rhs)),
            None => ExprKind::In(Box::new(lhs), Box::new(rhs)),
        };
        node(kind, p)
    }

    fn unary(&mut self) -> Result<Expr, ParseError> {
        if *self.peek() == Tok::Bang {
            self.enter()?;
            let p = self.bump().1;
            let e = self.unary()?;
            self.level -= 1;
            return node(ExprKind::Not(Box::new(e)), p);
        }
        self.postfix()
    }

    fn postfix(&mut self) -> Result<Expr, ParseError> {
        let mut e = self.primary()?;
        while *self.peek() == Tok::Dot {
            self.bump();
            let (t, p) = self.bump();
            let Tok::Ident(name) = t else {
                return Err(ParseError::syntax(p, "expected a field name after `.`"));
            };
            if *self.peek() == Tok::LParen {
                return Err(ParseError::syntax(p, format!("unknown function `{name}`")));
            }
            e = match e.kind {
                ExprKind::Path(mut path) => {
                    path.0.push(name);
                    Expr { kind: ExprKind::Path(path), pos: e.pos, depth: e.depth }
                }
                _ => node(ExprKind::Field(Box::new(e), name), p)?,
            };
        }
        Ok(e)
    }

    fn primary(&mut self) -> Result<Expr, ParseError> {
        let (t, p) = self.bump();
        match t {
            Tok::Int(n) => node(ExprKind::Int(n), p),
            Tok::Float(f) => node(ExprKind::Float(f), p),
            Tok::Str(s) => node(ExprKind::Str(s), p),
            Tok::LParen => {
                let e = self.expr()?;
                self.expect(&Tok::RParen, "`)`")?;
                Ok(e)
            }
            Tok::LBrack => {
                let mut items = Vec::new();
                if *self.peek() != Tok::RBrack {
                    loop {
                        items.push(self.expr()?);
                        if *self.peek() == Tok::Comma {
                            self.bump();
                        } else {
                            break;
                        }
                    }
                }
                self.expect(&Tok::RBrack, "`]`")?;
                node(ExprKind::List(items), p)
            }
            Tok::Ident(w) => self.ident(w, p),
            Tok::Eof => Err(ParseError::syntax(p, "unexpected end of expression")),
            _ => Err(ParseError::syntax(p, "unexpected token")),
        }
    }

    fn ident(&mut self, w: String, p: usize) -> Result<Expr, ParseError> {
        match w.as_str() {
            "true" => return node(ExprKind::Bool(true), p),
            "false" => return node(ExprKind::Bool(false), p),
            "in" => return Err(ParseError::syntax(p, "unexpected `in`")),
            word if FORBIDDEN.contains(&word) => {
                return Err(ParseError::syntax(p, format!("`{word}` is not part of FPL")));
            }
            _ => {}
        }
        if *self.peek() == Tok::LParen {
            self.bump();
            let e = match w.as_str() {
                "size" => {
                    let a = self.expr()?;
                    node(ExprKind::Size(Box::new(a)), p)?
                }
                "has" => {
                    let a = self.expr()?;
                    if !matches!(a.kind, ExprKind::Path(_) | ExprKind::Field(..)) {
                        return Err(ParseError::syntax(a.pos, "has() takes a field, e.g. has(params.x)"));
                    }
                    node(ExprKind::Has(Box::new(a)), p)?
                }
                "all" | "exists" => self.quant(w == "all", p)?,
                _ => return Err(ParseError::syntax(p, format!("unknown function `{w}`"))),
            };
            self.expect(&Tok::RParen, "`)`")?;
            return Ok(e);
        }
        if self.vars.contains(&w) {
            return node(ExprKind::Var(w), p);
        }
        node(ExprKind::Path(FieldPath(vec![w])), p)
    }

    fn quant(&mut self, is_all: bool, p: usize) -> Result<Expr, ParseError> {
        let (t, vp) = self.bump();
        let Tok::Ident(var) = t else {
            return Err(ParseError::syntax(vp, "expected a variable name"));
        };
        if matches!(var.as_str(), "true" | "false" | "in" | "size" | "has" | "all" | "exists")
            || FORBIDDEN.contains(&var.as_str())
        {
            return Err(ParseError::syntax(vp, format!("`{var}` cannot be a variable")));
        }
        if self.vars.contains(&var) {
            return Err(ParseError::syntax(vp, format!("variable `{var}` is already bound")));
        }
        if !matches!(self.peek(), Tok::Ident(w) if w == "in") {
            return Err(ParseError::syntax(self.pos(), "expected `in`"));
        }
        self.bump();
        let list = self.expr()?;
        self.expect(&Tok::Comma, "`,`")?;
        self.vars.push(var.clone());
        let body = self.expr();
        self.vars.pop();
        let body = body?;
        node(ExprKind::Quant { is_all, var, list: Box::new(list), body: Box::new(body) }, p)
    }
}

/// Parses an FPL expression.
pub fn parse(src: &str) -> Result<Expr, ParseError> {
    if src.len() > MAX_SRC_BYTES {
        return Err(ParseError::syntax(MAX_SRC_BYTES, format!("precondition longer than {MAX_SRC_BYTES} bytes")));
    }
    let toks = lex(src)?;
    let mut p = Parser { toks, i: 0, level: 0, vars: Vec::new() };
    let e = p.expr()?;
    if *p.peek() != Tok::Eof {
        return Err(ParseError::syntax(p.pos(), "unexpected token after expression"));
    }
    Ok(e)
}

// ---------------------------------------------------------------- types

/// Path-to-type map. Keys are dotted paths; `P[]` stands for the elements of list `P`, so
/// `params.items[].kind` types the field `kind` of each element of `params.items`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TypeEnv(pub BTreeMap<String, Type>);

/// Types of the `params` root, derived from the command's JSON Schema parameter schema.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParamSchema(pub TypeEnv);

impl ParamSchema {
    /// Maps `boolean`, `integer`, `number`, `string` (`ref` with `x-ref`), `array` (with
    /// `items`) and `object` (with `properties`) under the root `params`. Properties of any
    /// other shape are left untyped, so a precondition cannot read them.
    pub fn from_json_schema(schema: &CValue) -> Self {
        let mut env = BTreeMap::new();
        let mut work: Vec<(String, &CValue, usize)> = vec![("params".to_string(), schema, 0)];
        while let Some((key, s, depth)) = work.pop() {
            let Some(t) = schema_type(s) else { continue };
            env.insert(key.clone(), t);
            if depth >= MAX_SCHEMA_DEPTH {
                continue;
            }
            let CValue::Obj(o) = s else { continue };
            if let Some(CValue::Obj(props)) = o.get("properties") {
                for (name, sub) in props {
                    work.push((format!("{key}.{name}"), sub, depth + 1));
                }
            }
            if let Some(items) = o.get("items") {
                work.push((format!("{key}[]"), items, depth + 1));
            }
        }
        ParamSchema(TypeEnv(env))
    }
}

fn schema_type(s: &CValue) -> Option<Type> {
    let mut s = s;
    let mut lists = 0usize;
    loop {
        let CValue::Obj(o) = s else { return None };
        let ty = match o.get("type") {
            Some(CValue::Str(t)) => t.as_str(),
            None if o.contains_key("x-ref") => "string",
            _ => return None,
        };
        let mut t = match ty {
            "boolean" => Type::Bool,
            "integer" => Type::Int,
            "number" => Type::Float,
            "string" if o.contains_key("x-ref") => Type::EntityRef,
            "string" => Type::String,
            "object" => Type::Map,
            "array" => {
                lists += 1;
                if lists > MAX_SCHEMA_DEPTH {
                    return None;
                }
                s = o.get("items")?;
                continue;
            }
            _ => return None,
        };
        for _ in 0..lists {
            t = Type::List(Box::new(t));
        }
        return Some(t);
    }
}

/// Declared types of the non-`params` context roots (e.g. `doc.active.kind`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContextTypes(pub TypeEnv);

impl ContextTypes {
    pub fn new() -> Self {
        ContextTypes::default()
    }
    /// Declares the type of `path`. Paths under `params` are ignored: the parameter schema
    /// types them.
    pub fn with(mut self, path: &str, t: Type) -> Self {
        self.insert(path, t);
        self
    }
    pub fn insert(&mut self, path: &str, t: Type) {
        if root_of(path) != "params" {
            self.0 .0.insert(path.to_string(), t);
        }
    }
}

fn root_of(key: &str) -> &str {
    key.split(['.', '[']).next().unwrap_or("")
}

struct Checker<'a> {
    params: &'a ParamSchema,
    ctx: &'a ContextTypes,
    scope: Vec<(&'a str, Type, Option<String>)>,
}

impl<'a> Checker<'a> {
    fn lookup(&self, key: &str) -> Option<Type> {
        let env = if root_of(key) == "params" { &self.params.0 } else { &self.ctx.0 };
        env.0.get(key).cloned()
    }

    /// Returns the type and, where known, the path key that types the value's fields.
    /// Recursion depth is bounded by the AST depth limit.
    fn check(&mut self, e: &'a Expr) -> Result<(Type, Option<String>), ParseError> {
        let bad = |msg: String| Err(ParseError::ty(e.pos, msg));
        Ok(match &e.kind {
            ExprKind::Bool(_) => (Type::Bool, None),
            ExprKind::Int(_) => (Type::Int, None),
            ExprKind::Float(_) => (Type::Float, None),
            ExprKind::Str(_) => (Type::String, None),
            ExprKind::List(items) => {
                let mut elem: Option<Type> = None;
                for it in items {
                    let (t, _) = self.check(it)?;
                    elem = Some(match elem {
                        None => t,
                        Some(prev) => match join(&prev, &t) {
                            Some(j) => j,
                            None => return Err(ParseError::ty(it.pos, format!("list mixes {prev} and {t}"))),
                        },
                    });
                }
                match elem {
                    Some(t) => (Type::List(Box::new(t)), None),
                    None => return bad("an empty list literal has no element type".into()),
                }
            }
            ExprKind::Path(p) => {
                let key = p.to_string();
                match self.lookup(&key) {
                    Some(t) => (t, Some(key)),
                    None => return bad(format!("unknown field `{key}`")),
                }
            }
            ExprKind::Var(v) => match self.scope.iter().rev().find(|s| s.0 == v.as_str()) {
                Some((_, t, prefix)) => (t.clone(), prefix.clone()),
                None => return bad(format!("unknown variable `{v}`")),
            },
            ExprKind::Field(base, name) => {
                let (t, prefix) = self.check(base)?;
                if t != Type::Map {
                    return bad(format!("field access on {t}"));
                }
                let Some(prefix) = prefix else { return bad(format!("unknown field `{name}`")) };
                let key = format!("{prefix}.{name}");
                match self.lookup(&key) {
                    Some(t) => (t, Some(key)),
                    None => return bad(format!("unknown field `{key}`")),
                }
            }
            ExprKind::Not(a) => {
                self.want_bool(a)?;
                (Type::Bool, None)
            }
            ExprKind::And(items) | ExprKind::Or(items) => {
                for it in items {
                    self.want_bool(it)?;
                }
                (Type::Bool, None)
            }
            ExprKind::Cmp(op, a, b) => {
                let (ta, _) = self.check(a)?;
                let (tb, _) = self.check(b)?;
                let ok = match op {
                    CmpOp::Eq | CmpOp::Ne => eq_compatible(&ta, &tb),
                    _ => (ta.numeric() && tb.numeric()) || (ta == Type::String && tb == Type::String),
                };
                if !ok {
                    return bad(format!("cannot compare {ta} with {tb}"));
                }
                (Type::Bool, None)
            }
            ExprKind::In(a, b) => {
                let (ta, _) = self.check(a)?;
                let (tb, _) = self.check(b)?;
                let ok = match &tb {
                    Type::List(el) => eq_compatible(&ta, el),
                    Type::Map => ta == Type::String,
                    _ => false,
                };
                if !ok {
                    return bad(format!("cannot test {ta} in {tb}"));
                }
                (Type::Bool, None)
            }
            ExprKind::Cond(c, a, b) => {
                self.want_bool(c)?;
                let (ta, _) = self.check(a)?;
                let (tb, _) = self.check(b)?;
                match join(&ta, &tb) {
                    Some(t) => (t, None),
                    None => return bad(format!("branches have types {ta} and {tb}")),
                }
            }
            ExprKind::Size(a) => {
                let (t, _) = self.check(a)?;
                if !matches!(t, Type::List(_) | Type::Map | Type::String) {
                    return bad(format!("size() of {t}"));
                }
                (Type::Int, None)
            }
            ExprKind::Has(a) => {
                self.check(a)?;
                (Type::Bool, None)
            }
            ExprKind::Quant { var, list, body, .. } => {
                let (t, prefix) = self.check(list)?;
                let Type::List(el) = t else { return bad(format!("all()/exists() over {t}")) };
                self.scope.push((var.as_str(), *el, prefix.map(|p| format!("{p}[]"))));
                let r = self.want_bool(body);
                self.scope.pop();
                r?;
                (Type::Bool, None)
            }
        })
    }

    fn want_bool(&mut self, e: &'a Expr) -> Result<(), ParseError> {
        let (t, _) = self.check(e)?;
        if t != Type::Bool {
            return Err(ParseError::ty(e.pos, format!("expected bool, found {t}")));
        }
        Ok(())
    }
}

fn join(a: &Type, b: &Type) -> Option<Type> {
    if a == b {
        Some(a.clone())
    } else if a.numeric() && b.numeric() {
        Some(Type::Float)
    } else {
        None
    }
}

fn eq_compatible(a: &Type, b: &Type) -> bool {
    (a.numeric() && b.numeric()) || (a == b && a.scalar())
}

/// Type-checks a parsed expression against the parameter schema and context types; the
/// whole expression must be `bool` (REQ-002).
pub fn typecheck(e: &Expr, params: &ParamSchema, ctx: &ContextTypes) -> Result<(), ParseError> {
    let mut c = Checker { params, ctx, scope: Vec::new() };
    c.want_bool(e)
}

// ---------------------------------------------------------------- predicates

/// A checked precondition.
#[derive(Debug, Clone, PartialEq)]
pub struct Predicate {
    /// The expression source.
    pub src: String,
    pub ast: Expr,
    /// Step bound, 1..=[`MAX_BOUND`].
    pub bound: u32,
    /// Message key returned when the predicate is false.
    pub message_key: String,
}

impl Predicate {
    /// Parses and type-checks `expr`. A bound outside 1..=[`MAX_BOUND`] is `fpl_bound`.
    pub fn compile(
        expr: &str,
        bound: u32,
        message_key: &str,
        params: &ParamSchema,
        ctx: &ContextTypes,
    ) -> Result<Predicate, ParseError> {
        if !(1..=MAX_BOUND).contains(&bound) {
            return Err(ParseError {
                code: "fpl_bound",
                pos: 0,
                message: format!("step bound {bound} is outside 1..={MAX_BOUND}"),
            });
        }
        let ast = parse(expr)?;
        typecheck(&ast, params, ctx)?;
        Ok(Predicate { src: expr.to_string(), ast, bound, message_key: message_key.to_string() })
    }

    /// Compiles one declared precondition.
    pub fn from_decl(decl: &Precondition, params: &ParamSchema, ctx: &ContextTypes) -> Result<Predicate, ParseError> {
        Predicate::compile(&decl.expr, decl.bound, &decl.message_key, params, ctx)
    }
}

/// A registry-build failure: which command, which precondition, and the parse/type error
/// with its byte position in the expression (REQ-002).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildError {
    /// `id@major`.
    pub command: String,
    /// Index into `CommandDef::preconditions`.
    pub index: usize,
    pub error: ParseError,
}

impl fmt::Display for BuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "command {} precondition {}: {}", self.command, self.index, self.error)
    }
}

impl std::error::Error for BuildError {}

/// Compiles every precondition of a command for the registry build.
pub fn compile_command(def: &CommandDef, ctx: &ContextTypes) -> Result<Vec<Predicate>, BuildError> {
    let params = ParamSchema::from_json_schema(&def.params_schema);
    def.preconditions
        .iter()
        .enumerate()
        .map(|(index, decl)| {
            Predicate::from_decl(decl, &params, ctx).map_err(|error| BuildError {
                command: format!("{}@{}", def.id, def.major),
                index,
                error,
            })
        })
        .collect()
}

// ---------------------------------------------------------------- evaluator

/// A completed evaluation that did not error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    True,
    False { message_key: String },
}

/// Result and step count of one evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Evaluation {
    pub result: Result<Verdict, PredError>,
    pub steps: u32,
}

/// Progress of one `all`/`exists`.
struct Quant<'a> {
    is_all: bool,
    var: &'a str,
    body: &'a Expr,
    pos: usize,
    items: Vec<Value>,
    idx: usize,
    /// Display path of the list, for error messages.
    origin: String,
}

enum Task<'a> {
    Visit(&'a Expr),
    Not(usize),
    /// Next operand of an n-ary `&&` (`is_and`) or `||`.
    Chain {
        is_and: bool,
        items: &'a [Expr],
        idx: usize,
        pos: usize,
    },
    Cmp(CmpOp, usize),
    In(usize),
    Cond(&'a Expr, &'a Expr, usize),
    Size(usize),
    /// Field access; carries the field node to name the full path when it is missing.
    Field(&'a Expr, &'a str, usize),
    HasField(&'a str, usize),
    MakeList(usize),
    QuantList {
        is_all: bool,
        var: &'a str,
        list: &'a Expr,
        body: &'a Expr,
        pos: usize,
    },
    QuantNext(Quant<'a>),
    QuantAfter(Quant<'a>),
}

struct Machine<'a> {
    ctx: &'a dyn PredContext,
    bound: u32,
    steps: u32,
    vals: Vec<Value>,
    /// Bound variables: name, value, display path (e.g. `params.items[1]`).
    env: Vec<(&'a str, Value, String)>,
}

fn type_err(pos: usize, detail: String) -> PredError {
    PredError::TypeError { pos, detail }
}

/// `-0.0` becomes `+0.0`; every other value is unchanged.
fn positive_zero(f: f64) -> f64 {
    if f.to_bits() == (-0.0f64).to_bits() {
        0.0
    } else {
        f
    }
}

/// Normalises every float in a value read from the context, without recursion.
fn normalize(v: &mut Value) {
    let mut stack: Vec<&mut Value> = vec![v];
    while let Some(v) = stack.pop() {
        match v {
            Value::Float(f) => *f = positive_zero(*f),
            Value::List(items) => stack.extend(items.iter_mut()),
            Value::Map(m) => stack.extend(m.values_mut()),
            _ => {}
        }
    }
}

impl<'a> Machine<'a> {
    fn tick(&mut self) -> Result<(), PredError> {
        if self.steps >= self.bound {
            return Err(PredError::Budget { steps: self.steps });
        }
        self.steps += 1;
        Ok(())
    }

    fn pop(&mut self, pos: usize) -> Result<Value, PredError> {
        self.vals.pop().ok_or_else(|| type_err(pos, "evaluator stack underflow".into()))
    }

    fn pop_bool(&mut self, pos: usize) -> Result<bool, PredError> {
        match self.pop(pos)? {
            Value::Bool(b) => Ok(b),
            v => Err(type_err(pos, format!("expected bool, found {}", v.kind()))),
        }
    }

    /// Display path of a path, variable or field chain, e.g. `params.items[1].n`.
    /// Walks the chain iteratively.
    fn path_of(&self, e: &Expr) -> String {
        let mut names = Vec::new();
        let mut cur = e;
        while let ExprKind::Field(base, name) = &cur.kind {
            names.push(name.as_str());
            cur = base.as_ref();
        }
        let mut out = match &cur.kind {
            ExprKind::Path(p) => p.to_string(),
            ExprKind::Var(v) => match self.env.iter().rev().find(|(n, _, _)| *n == v.as_str()) {
                Some((_, _, origin)) => origin.clone(),
                None => v.clone(),
            },
            _ => "<value>".to_string(),
        };
        for n in names.iter().rev() {
            out.push('.');
            out.push_str(n);
        }
        out
    }

    fn run(&mut self, root: &'a Expr) -> Result<bool, PredError> {
        let mut work: Vec<Task<'a>> = vec![Task::Visit(root)];
        while let Some(task) = work.pop() {
            match task {
                Task::Visit(e) => {
                    self.tick()?;
                    self.visit(e, &mut work)?;
                }
                Task::Not(pos) => {
                    let b = self.pop_bool(pos)?;
                    self.vals.push(Value::Bool(!b));
                }
                Task::Chain { is_and, items, idx, pos } => {
                    let b = self.pop_bool(pos)?;
                    if b != is_and || idx >= items.len() {
                        // `false` ends an `&&`, `true` ends an `||`; or the chain is done.
                        self.vals.push(Value::Bool(b));
                    } else if let Some(next) = items.get(idx) {
                        work.push(Task::Chain { is_and, items, idx: idx + 1, pos });
                        work.push(Task::Visit(next));
                    }
                }
                Task::Cmp(op, pos) => {
                    let b = self.pop(pos)?;
                    let a = self.pop(pos)?;
                    let r = match op {
                        CmpOp::Eq => equals(&a, &b, pos)?,
                        CmpOp::Ne => !equals(&a, &b, pos)?,
                        CmpOp::Lt => order(&a, &b, pos)? == Ordering::Less,
                        CmpOp::Le => order(&a, &b, pos)? != Ordering::Greater,
                        CmpOp::Gt => order(&a, &b, pos)? == Ordering::Greater,
                        CmpOp::Ge => order(&a, &b, pos)? != Ordering::Less,
                    };
                    self.vals.push(Value::Bool(r));
                }
                Task::In(pos) => {
                    let b = self.pop(pos)?;
                    let a = self.pop(pos)?;
                    let r = match (&a, &b) {
                        (_, Value::List(items)) => {
                            let mut found = false;
                            for it in items {
                                if equals(&a, it, pos)? {
                                    found = true;
                                    break;
                                }
                            }
                            found
                        }
                        (Value::Str(k), Value::Map(m)) => m.contains_key(k),
                        _ => return Err(type_err(pos, format!("cannot test {} in {}", a.kind(), b.kind()))),
                    };
                    self.vals.push(Value::Bool(r));
                }
                Task::Cond(a, b, pos) => {
                    let c = self.pop_bool(pos)?;
                    work.push(Task::Visit(if c { a } else { b }));
                }
                Task::Size(pos) => {
                    let n = match self.pop(pos)? {
                        Value::List(l) => l.len(),
                        Value::Map(m) => m.len(),
                        Value::Str(s) => s.chars().count(),
                        v => return Err(type_err(pos, format!("size() of {}", v.kind()))),
                    };
                    self.vals.push(Value::Int(i64::try_from(n).unwrap_or(i64::MAX)));
                }
                Task::Field(node, name, pos) => match self.pop(pos)? {
                    Value::Map(mut m) => match m.remove(name) {
                        Some(v) => self.vals.push(v),
                        None => return Err(PredError::MissingField { path: self.path_of(node) }),
                    },
                    v => return Err(type_err(pos, format!("field access on {}", v.kind()))),
                },
                Task::HasField(name, pos) => match self.pop(pos)? {
                    Value::Map(m) => self.vals.push(Value::Bool(m.contains_key(name))),
                    v => return Err(type_err(pos, format!("has() on {}", v.kind()))),
                },
                Task::MakeList(n) => {
                    let at = self.vals.len().saturating_sub(n);
                    let items = self.vals.split_off(at);
                    self.vals.push(Value::List(items));
                }
                Task::QuantList { is_all, var, list, body, pos } => match self.pop(pos)? {
                    Value::List(items) => {
                        let origin = self.path_of(list);
                        work.push(Task::QuantNext(Quant { is_all, var, body, pos, items, idx: 0, origin }));
                    }
                    v => return Err(type_err(pos, format!("all()/exists() over {}", v.kind()))),
                },
                Task::QuantNext(q) => {
                    let Some(item) = q.items.get(q.idx).cloned() else {
                        self.vals.push(Value::Bool(q.is_all));
                        continue;
                    };
                    self.tick()?;
                    self.env.push((q.var, item, format!("{}[{}]", q.origin, q.idx)));
                    let body = q.body;
                    work.push(Task::QuantAfter(q));
                    work.push(Task::Visit(body));
                }
                Task::QuantAfter(mut q) => {
                    let b = self.pop_bool(q.pos)?;
                    self.env.pop();
                    if b != q.is_all {
                        // `all` stops at the first false element, `exists` at the first true one.
                        self.vals.push(Value::Bool(b));
                    } else {
                        q.idx += 1;
                        work.push(Task::QuantNext(q));
                    }
                }
            }
        }
        self.pop_bool(root.pos)
    }

    fn visit(&mut self, e: &'a Expr, work: &mut Vec<Task<'a>>) -> Result<(), PredError> {
        match &e.kind {
            ExprKind::Bool(b) => self.vals.push(Value::Bool(*b)),
            ExprKind::Int(n) => self.vals.push(Value::Int(*n)),
            ExprKind::Float(f) => self.vals.push(Value::Float(positive_zero(*f))),
            ExprKind::Str(s) => self.vals.push(Value::Str(s.clone())),
            ExprKind::List(items) => {
                work.push(Task::MakeList(items.len()));
                for it in items.iter().rev() {
                    work.push(Task::Visit(it));
                }
            }
            ExprKind::Path(p) => {
                let mut v = self.ctx.get(p)?;
                normalize(&mut v);
                self.vals.push(v);
            }
            ExprKind::Var(name) => match self.env.iter().rev().find(|(n, _, _)| *n == name.as_str()) {
                Some((_, v, _)) => self.vals.push(v.clone()),
                None => return Err(type_err(e.pos, format!("unbound variable `{name}`"))),
            },
            ExprKind::Field(base, name) => {
                work.push(Task::Field(e, name, e.pos));
                work.push(Task::Visit(base));
            }
            ExprKind::Not(a) => {
                work.push(Task::Not(e.pos));
                work.push(Task::Visit(a));
            }
            ExprKind::And(items) | ExprKind::Or(items) => {
                let is_and = matches!(e.kind, ExprKind::And(_));
                if let Some(first) = items.first() {
                    work.push(Task::Chain { is_and, items, idx: 1, pos: e.pos });
                    work.push(Task::Visit(first));
                } else {
                    self.vals.push(Value::Bool(is_and));
                }
            }
            ExprKind::Cmp(op, a, b) => {
                work.push(Task::Cmp(*op, e.pos));
                work.push(Task::Visit(b));
                work.push(Task::Visit(a));
            }
            ExprKind::In(a, b) => {
                work.push(Task::In(e.pos));
                work.push(Task::Visit(b));
                work.push(Task::Visit(a));
            }
            ExprKind::Cond(c, a, b) => {
                work.push(Task::Cond(a, b, e.pos));
                work.push(Task::Visit(c));
            }
            ExprKind::Size(a) => {
                work.push(Task::Size(e.pos));
                work.push(Task::Visit(a));
            }
            ExprKind::Has(a) => match &a.kind {
                ExprKind::Path(p) => {
                    self.tick()?;
                    let present = match self.ctx.get(p) {
                        Ok(_) => true,
                        Err(PredError::MissingField { .. }) => false,
                        Err(other) => return Err(other),
                    };
                    self.vals.push(Value::Bool(present));
                }
                ExprKind::Field(base, name) => {
                    self.tick()?;
                    work.push(Task::HasField(name, a.pos));
                    work.push(Task::Visit(base));
                }
                _ => return Err(type_err(a.pos, "has() takes a field".into())),
            },
            ExprKind::Quant { is_all, var, list, body } => {
                work.push(Task::QuantList { is_all: *is_all, var: var.as_str(), list, body, pos: e.pos });
                work.push(Task::Visit(list));
            }
        }
        Ok(())
    }
}

/// Exact numeric order; NaN is `nan_operand` (REQ-007). Floats use IEEE 754 total order
/// (`-0.0` never reaches here: it is normalised on entry); an integer and a float compare
/// by exact value.
fn num_cmp(a: &Value, b: &Value, pos: usize) -> Option<Result<Ordering, PredError>> {
    let nan = |f: &f64| f.is_nan();
    Some(match (a, b) {
        (Value::Int(x), Value::Int(y)) => Ok(x.cmp(y)),
        (Value::Float(x), Value::Float(y)) => {
            if nan(x) || nan(y) {
                Err(PredError::NanOperand { pos })
            } else {
                Ok(x.total_cmp(y))
            }
        }
        (Value::Int(x), Value::Float(y)) => {
            if nan(y) {
                Err(PredError::NanOperand { pos })
            } else {
                Ok(int_float(*x, *y))
            }
        }
        (Value::Float(x), Value::Int(y)) => {
            if nan(x) {
                Err(PredError::NanOperand { pos })
            } else {
                Ok(int_float(*y, *x).reverse())
            }
        }
        _ => return None,
    })
}

/// Exact comparison of an integer with a non-NaN float.
fn int_float(i: i64, f: f64) -> Ordering {
    const TWO63: f64 = 9_223_372_036_854_775_808.0;
    if f >= TWO63 {
        return Ordering::Less;
    }
    if f < -TWO63 {
        return Ordering::Greater;
    }
    let t = f.trunc();
    // |t| < 2^63 and t is integral, so the cast is exact.
    match i.cmp(&(t as i64)) {
        Ordering::Equal => {
            let frac = f - t;
            if frac > 0.0 {
                Ordering::Less
            } else if frac < 0.0 {
                Ordering::Greater
            } else {
                Ordering::Equal
            }
        }
        o => o,
    }
}

fn equals(a: &Value, b: &Value, pos: usize) -> Result<bool, PredError> {
    if let Some(r) = num_cmp(a, b, pos) {
        return Ok(r? == Ordering::Equal);
    }
    match (a, b) {
        (Value::Bool(x), Value::Bool(y)) => Ok(x == y),
        (Value::Str(x), Value::Str(y)) | (Value::Ref(x), Value::Ref(y)) => Ok(x == y),
        _ => Err(type_err(pos, format!("cannot compare {} with {}", a.kind(), b.kind()))),
    }
}

fn order(a: &Value, b: &Value, pos: usize) -> Result<Ordering, PredError> {
    if let Some(r) = num_cmp(a, b, pos) {
        return r;
    }
    match (a, b) {
        (Value::Str(x), Value::Str(y)) => Ok(x.as_bytes().cmp(y.as_bytes())),
        _ => Err(type_err(pos, format!("cannot order {} and {}", a.kind(), b.kind()))),
    }
}

/// Evaluates a predicate and reports the step count (REQ-003..REQ-005). Never panics.
pub fn evaluate(pred: &Predicate, ctx: &dyn PredContext) -> Evaluation {
    let bound = pred.bound.min(MAX_BOUND);
    let mut m = Machine { ctx, bound, steps: 0, vals: Vec::new(), env: Vec::new() };
    let result = match m.run(&pred.ast) {
        Ok(true) => Ok(Verdict::True),
        Ok(false) => Ok(Verdict::False { message_key: pred.message_key.clone() }),
        Err(e) => Err(e),
    };
    Evaluation { result, steps: m.steps }
}

/// Evaluates a predicate to a boolean.
pub fn eval(pred: &Predicate, ctx: &dyn PredContext) -> Result<bool, PredError> {
    evaluate(pred, ctx).result.map(|v| v == Verdict::True)
}
