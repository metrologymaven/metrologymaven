//! Expression grammar, bounded parser and printer. Spec node `AUTO.PROG.EXPR.GRAMMAR`.
//!
//! The language is closed and small (EBNF in `docs/program/expr-grammar.md`): literals
//! (integer, decimal, quantity `12.700[mm]`, string, boolean), identifiers, `.field` and
//! `[index]` access, unary `-` and `not`, the binary operators `* / %`, `+ -`,
//! `< <= > >= == !=`, `and`, `or` (in falling precedence, all left-associative),
//! `if c then a else b`, and calls `name(args)`. There is no assignment, declaration,
//! loop, I/O or way to name a registry command, so an expression cannot change state.
//!
//! All input is hostile. [`parse_expr`] refuses sources above [`MAX_SOURCE_BYTES`],
//! [`MAX_TOKENS`] or nesting depth [`MAX_DEPTH`] with [`ParseError::TooLarge`] before
//! building anything deeper, and reports every other failure as [`ParseError::Syntax`] with
//! the byte offset of the first offending token. Recursion in the parser is bounded by an
//! explicit depth counter; the printer and [`Expr::same_tree`] use explicit stacks.

mod lex;
mod parse;
mod print;

pub use print::print_expr;

/// Largest accepted source, in bytes (REQ-AUTO.PROG.EXPR.GRAMMAR-002).
pub const MAX_SOURCE_BYTES: usize = 4 * 1024;
/// Most tokens in one source (REQ-AUTO.PROG.EXPR.GRAMMAR-002).
pub const MAX_TOKENS: usize = 1024;
/// Deepest accepted nesting (REQ-AUTO.PROG.EXPR.GRAMMAR-002). Every AST edge and every pair
/// of parentheses is one level, so `x` has depth 0, `(x)` and `-x` depth 1, and `a+b+c` depth 2.
pub const MAX_DEPTH: usize = 64;
/// Longest identifier, in bytes; the same limit as program identifiers in `AUTO.PROG.FMT`.
pub const MAX_NAME_BYTES: usize = 64;

/// Reserved words; none of them is an identifier.
pub const KEYWORDS: [&str; 8] = ["and", "else", "false", "if", "not", "or", "then", "true"];

/// Parse an expression source into a syntax tree with byte spans.
pub fn parse_expr(src: &str) -> Result<Expr, ParseError> {
    if src.len() > MAX_SOURCE_BYTES {
        return Err(ParseError::TooLarge(Limit::Bytes));
    }
    // The byte and token bounds win over any syntax error. Otherwise errors are reported in
    // source order: the tokens before a lexical error are parsed first, with the error's
    // position as their end, and the lexical error wins unless the parser fails earlier.
    let (tokens, lex_err) = lex::lex(src);
    let stop = match lex_err {
        Some(ParseError::Syntax { offset }) => offset,
        Some(too_large) => return Err(too_large),
        None => src.len(),
    };
    match (parse::parse(&tokens, stop), lex_err) {
        (Ok(e), None) => Ok(e),
        (Err(ParseError::Syntax { offset }), Some(l)) if offset >= stop => Err(l),
        (Err(e), _) => Err(e),
        (Ok(_), Some(l)) => Err(l),
    }
}

/// Half-open byte range `start..end` in the source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Span {
        Span { start, end }
    }
}

/// An expression node and the source bytes it covers. A parenthesised expression's span
/// includes its parentheses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

/// The node kinds of the grammar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExprKind {
    /// Integer literal. The parser yields `0..=i64::MAX`; a minus sign is a [`UnaryOp::Neg`].
    Int(i64),
    /// Decimal literal `<digits>.<digits>`, text kept verbatim.
    Dec(DecText),
    /// Quantity literal `<number>[<unit symbol>]`; number text kept verbatim, unit unresolved.
    Qty(DecText, UnitSym),
    /// String literal, escapes resolved.
    Str(String),
    Bool(bool),
    /// Variable or parameter reference.
    Ident(Name),
    /// `base.name`
    Field(Box<Expr>, Name),
    /// `base[index]`
    Index(Box<Expr>, Box<Expr>),
    Unary(UnaryOp, Box<Expr>),
    Binary(BinaryOp, Box<Expr>, Box<Expr>),
    /// `if cond then a else b`
    Cond(Box<Expr>, Box<Expr>, Box<Expr>),
    /// `name(args)`: a function of the fixed library (resolved by `AUTO.PROG.EXPR.TYPES`).
    Call(Name, Vec<Expr>),
}

/// Unary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum UnaryOp {
    /// `-`
    Neg,
    /// `not`
    Not,
}

impl UnaryOp {
    /// Source text.
    pub fn as_str(self) -> &'static str {
        match self {
            UnaryOp::Neg => "-",
            UnaryOp::Not => "not",
        }
    }
}

/// Binary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum BinaryOp {
    Mul,
    Div,
    Rem,
    Add,
    Sub,
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
    And,
    Or,
}

impl BinaryOp {
    /// All operators, for generators and tables.
    pub const ALL: [BinaryOp; 13] = [
        BinaryOp::Mul,
        BinaryOp::Div,
        BinaryOp::Rem,
        BinaryOp::Add,
        BinaryOp::Sub,
        BinaryOp::Lt,
        BinaryOp::Le,
        BinaryOp::Gt,
        BinaryOp::Ge,
        BinaryOp::Eq,
        BinaryOp::Ne,
        BinaryOp::And,
        BinaryOp::Or,
    ];

    /// Source text.
    pub fn as_str(self) -> &'static str {
        match self {
            BinaryOp::Mul => "*",
            BinaryOp::Div => "/",
            BinaryOp::Rem => "%",
            BinaryOp::Add => "+",
            BinaryOp::Sub => "-",
            BinaryOp::Lt => "<",
            BinaryOp::Le => "<=",
            BinaryOp::Gt => ">",
            BinaryOp::Ge => ">=",
            BinaryOp::Eq => "==",
            BinaryOp::Ne => "!=",
            BinaryOp::And => "and",
            BinaryOp::Or => "or",
        }
    }

    /// Binding power: 5 for `* / %`, 4 for `+ -`, 3 for comparisons, 2 for `and`, 1 for `or`.
    /// Higher binds tighter; every level is left-associative.
    pub fn precedence(self) -> u8 {
        match self {
            BinaryOp::Mul | BinaryOp::Div | BinaryOp::Rem => 5,
            BinaryOp::Add | BinaryOp::Sub => 4,
            BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge | BinaryOp::Eq | BinaryOp::Ne => 3,
            BinaryOp::And => 2,
            BinaryOp::Or => 1,
        }
    }
}

fn is_name(s: &str) -> bool {
    let b = s.as_bytes();
    match b.first() {
        Some(c) if c.is_ascii_alphabetic() || *c == b'_' => {}
        _ => return false,
    }
    b.len() <= MAX_NAME_BYTES && b.iter().all(|c| c.is_ascii_alphanumeric() || *c == b'_') && !KEYWORDS.contains(&s)
}

/// An identifier: `[A-Za-z_][A-Za-z0-9_]*`, at most [`MAX_NAME_BYTES`], not a keyword.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Name(String);

impl Name {
    /// `None` unless `s` is a valid identifier.
    pub fn new(s: &str) -> Option<Name> {
        is_name(s).then(|| Name(s.to_string()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Number text kept verbatim: `<digits>` or `<digits>.<digits>`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DecText(String);

impl DecText {
    /// `None` unless `s` is `<digits>` or `<digits>.<digits>` (ASCII digits).
    pub fn new(s: &str) -> Option<DecText> {
        let digits = |t: &str| !t.is_empty() && t.bytes().all(|c| c.is_ascii_digit());
        let ok = match s.split_once('.') {
            Some((i, f)) => digits(i) && digits(f),
            None => digits(s),
        };
        ok.then(|| DecText(s.to_string()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
    /// Whether the text has a fractional part (`Dec` literals always do).
    pub fn has_point(&self) -> bool {
        self.0.contains('.')
    }
}

/// Whether `c` may appear in a unit symbol: ASCII letters and digits, `_ . / ^ * - %`,
/// and the four non-ASCII symbols `µ` (U+00B5), `μ` (U+03BC), `°` (U+00B0), `Ω` (U+03A9).
pub fn is_unit_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '/' | '^' | '*' | '-' | '%' | 'µ' | 'μ' | '°' | 'Ω')
}

/// A unit symbol as written inside `[...]`; resolved against the unit catalogue by
/// `AUTO.PROG.EXPR.TYPES`, not here.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct UnitSym(String);

impl UnitSym {
    /// `None` unless `s` is non-empty and every character passes [`is_unit_char`].
    pub fn new(s: &str) -> Option<UnitSym> {
        (!s.is_empty() && s.chars().all(is_unit_char)).then(|| UnitSym(s.to_string()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Expr {
    pub fn new(kind: ExprKind, span: Span) -> Expr {
        Expr { kind, span }
    }

    /// Whether two trees are equal ignoring spans (print-then-parse identity,
    /// REQ-AUTO.PROG.EXPR.GRAMMAR-006). Uses an explicit stack, so any depth is safe.
    pub fn same_tree(&self, other: &Expr) -> bool {
        let mut stack: Vec<(&Expr, &Expr)> = vec![(self, other)];
        while let Some((a, b)) = stack.pop() {
            use ExprKind as K;
            match (&a.kind, &b.kind) {
                (K::Int(x), K::Int(y)) if x == y => {}
                (K::Dec(x), K::Dec(y)) if x == y => {}
                (K::Qty(x, u), K::Qty(y, v)) if x == y && u == v => {}
                (K::Str(x), K::Str(y)) if x == y => {}
                (K::Bool(x), K::Bool(y)) if x == y => {}
                (K::Ident(x), K::Ident(y)) if x == y => {}
                (K::Field(x, n), K::Field(y, m)) if n == m => stack.push((&**x, &**y)),
                (K::Index(x, i), K::Index(y, j)) => {
                    stack.push((&**x, &**y));
                    stack.push((&**i, &**j));
                }
                (K::Unary(o, x), K::Unary(p, y)) if o == p => stack.push((&**x, &**y)),
                (K::Binary(o, x, i), K::Binary(p, y, j)) if o == p => {
                    stack.push((&**x, &**y));
                    stack.push((&**i, &**j));
                }
                (K::Cond(c, x, i), K::Cond(d, y, j)) => {
                    stack.push((&**c, &**d));
                    stack.push((&**x, &**y));
                    stack.push((&**i, &**j));
                }
                (K::Call(n, xs), K::Call(m, ys)) if n == m && xs.len() == ys.len() => {
                    stack.extend(xs.iter().zip(ys.iter()));
                }
                _ => return false,
            }
        }
        true
    }
}

/// Which bound of REQ-AUTO.PROG.EXPR.GRAMMAR-002 a source exceeded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Limit {
    /// More than [`MAX_SOURCE_BYTES`].
    Bytes,
    /// More than [`MAX_TOKENS`].
    Tokens,
    /// Nesting deeper than [`MAX_DEPTH`].
    Depth,
}

impl Limit {
    /// Stable name.
    pub fn as_str(self) -> &'static str {
        match self {
            Limit::Bytes => "bytes",
            Limit::Tokens => "tokens",
            Limit::Depth => "depth",
        }
    }
}

/// Parse errors with stable codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseError {
    /// `expr_syntax`: the source does not parse; `offset` is the byte offset of the first
    /// offending token (the source length if the source ends too early).
    Syntax { offset: usize },
    /// `expr_too_large`: a bound was exceeded; no tree is returned.
    TooLarge(Limit),
}

impl ParseError {
    /// Stable machine-readable code.
    pub fn code(&self) -> &'static str {
        match self {
            ParseError::Syntax { .. } => "expr_syntax",
            ParseError::TooLarge(_) => "expr_too_large",
        }
    }
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::Syntax { offset } => write!(f, "expr_syntax at byte {offset}"),
            ParseError::TooLarge(l) => write!(f, "expr_too_large: {}", l.as_str()),
        }
    }
}

impl std::error::Error for ParseError {}

pub(crate) fn syntax(offset: usize) -> ParseError {
    ParseError::Syntax { offset }
}
