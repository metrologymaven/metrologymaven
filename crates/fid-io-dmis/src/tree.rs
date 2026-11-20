//! The generic, lossless DMIS statement tree (spec node `IO.DMIS.PARSE.TREE`).
//!
//! [`parse`] lexes the input ([`crate::lex`]) and turns each logical statement into one
//! [`Node`]. The rules below are the project's own reading of the leaf spec, not text from
//! the DMIS standard:
//!
//! - A logical statement that is one comment line becomes [`Node::Comment`].
//! - **Head**: `label = WORD ...` (a label token, `=`, a word) or `WORD ...`. The word is the
//!   major word. Any other start gives [`Node::Unknown`] with reason
//!   [`UnknownReason::NoMajorWord`].
//! - A major word with no descriptor in the [`Registry`] gives [`Node::Unknown`] holding the
//!   keyword and every raw token (no reason), and parsing goes on.
//! - **Structure** of a registered statement: after the major word either nothing or `/`
//!   followed by fields. Fields are separated by `,`; `( ... )` opens a nested group whose own
//!   fields are separated the same way, at most [`MAX_GROUP_DEPTH`] deep
//!   (`limit_exceeded{group_depth}`). An empty field list (nothing after `/` or between
//!   parentheses) has zero fields; otherwise there is one field more than there are commas,
//!   so `A/1,` has an empty second field. Any other token after the major word, a `)` with no
//!   open group, or a group still open at the end is a shape mismatch.
//! - **Shape**: the descriptors of the major word are tried in registration order; the first
//!   whose label rule and shape match gives a [`Statement`] of its class. If none matches,
//!   parsing fails with `malformed{line, col, class}`, or, with [`ParseOptions::lenient`],
//!   the statement becomes [`Node::Unknown`] with reason [`UnknownReason::ShapeMismatch`].
//!   The reported class is the descriptor that got farthest through the fields (the first
//!   one on a tie; one whose label rule failed counts as getting nowhere), and the position
//!   is the first token of the field where it stopped, or the statement's last token when
//!   it ran out of fields. For a structure error it is the first descriptor of the major word
//!   and the offending token.
//! - Comments inside a continuation chain are kept as [`Trivia`] of the statement.
//!
//! The tree is lossless: [`DmisTree::tokens`] returns every token of the input except
//! whitespace and continuation markers, in input order, with its authored spelling and
//! position. A failed parse returns only the error, never a partial tree.

use crate::lex::{lex_slice, LexError, LexOptions, LogicalStatement, Pos, Token, TokenKind};
use crate::registry::{Dialect, Registry, RegistryError, StatementDescriptor};
use fid_io_reg::{Budget, LimitRequest, Principal};
use std::fmt;

/// Deepest nesting of parenthesised groups in one statement.
pub const MAX_GROUP_DEPTH: usize = 32;

/// A label prefix: the label token and the `=` after it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Label {
    /// The label token, such as `F(P1)`.
    pub token: Token,
    /// The `=` token.
    pub equals: Token,
}

impl Label {
    /// The label prefix in upper case, such as `F`.
    pub fn prefix(&self) -> &str {
        match &self.token.kind {
            TokenKind::Label { prefix, .. } => prefix,
            _ => "",
        }
    }

    /// The label name as authored, such as `P1`.
    pub fn name(&self) -> &str {
        match &self.token.kind {
            TokenKind::Label { name, .. } => name,
            _ => "",
        }
    }
}

/// A parenthesised group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    /// The `(` token.
    pub open: Token,
    /// The fields inside.
    pub args: Vec<Arg>,
    /// The `)` token.
    pub close: Token,
}

/// One item of a field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    /// A word, number (lexeme kept), string, label reference, raw text, `=` or `/`.
    Atom(Token),
    /// A nested group.
    Group(Group),
}

impl Item {
    /// Position of the item's first token.
    pub fn pos(&self) -> Pos {
        match self {
            Item::Atom(t) => t.pos,
            Item::Group(g) => g.open.pos,
        }
    }
}

/// One comma-separated field: usually one item; empty for an empty field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Arg {
    /// The items of the field, in order.
    pub items: Vec<Item>,
    /// The `,` that ends the field, if any.
    pub comma: Option<Token>,
}

impl Arg {
    /// The item if the field holds exactly one.
    pub fn single(&self) -> Option<&Item> {
        match self.items.as_slice() {
            [one] => Some(one),
            _ => None,
        }
    }

    /// The token if the field is exactly one non-group token.
    pub fn atom(&self) -> Option<&Token> {
        match self.single() {
            Some(Item::Atom(t)) => Some(t),
            _ => None,
        }
    }
}

/// A comment inside a statement's continuation chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trivia {
    /// Number of non-comment tokens of the statement that come before it.
    pub before: usize,
    /// The comment token.
    pub token: Token,
}

/// A statement that matched a descriptor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Statement {
    /// Line and column of the statement's first token.
    pub pos: Pos,
    /// The label prefix, if any.
    pub label: Option<Label>,
    /// The major word token.
    pub major: Token,
    /// The `/` after the major word, if any.
    pub slash: Option<Token>,
    /// The fields after the slash.
    pub args: Vec<Arg>,
    /// The class ID of the descriptor that matched.
    pub class: Box<str>,
    /// Comments inside the statement.
    pub trivia: Vec<Trivia>,
}

impl Statement {
    /// The major word in upper case.
    pub fn major_word(&self) -> &str {
        match &self.major.kind {
            TokenKind::Word { upper } => upper,
            _ => "",
        }
    }

    /// Every token of the statement in input order, comments included.
    pub fn tokens(&self) -> Vec<&Token> {
        let mut sig: Vec<&Token> = Vec::new();
        if let Some(l) = &self.label {
            sig.push(&l.token);
            sig.push(&l.equals);
        }
        sig.push(&self.major);
        if let Some(s) = &self.slash {
            sig.push(s);
        }
        push_args(&self.args, &mut sig);
        let mut out = Vec::with_capacity(sig.len() + self.trivia.len());
        let mut trivia = self.trivia.iter().peekable();
        for (k, t) in sig.into_iter().enumerate() {
            while let Some(c) = trivia.next_if(|c| c.before <= k) {
                out.push(&c.token);
            }
            out.push(t);
        }
        out.extend(trivia.map(|c| &c.token));
        out
    }
}

enum Walk<'a> {
    Args(&'a [Arg]),
    Tok(&'a Token),
}

/// Appends the tokens of `args` in order, with an explicit stack instead of recursion.
fn push_args<'a>(args: &'a [Arg], out: &mut Vec<&'a Token>) {
    let mut stack = vec![Walk::Args(args)];
    while let Some(step) = stack.pop() {
        match step {
            Walk::Tok(t) => out.push(t),
            Walk::Args(list) => {
                for arg in list.iter().rev() {
                    if let Some(c) = &arg.comma {
                        stack.push(Walk::Tok(c));
                    }
                    for item in arg.items.iter().rev() {
                        match item {
                            Item::Atom(t) => stack.push(Walk::Tok(t)),
                            Item::Group(g) => {
                                stack.push(Walk::Tok(&g.close));
                                stack.push(Walk::Args(&g.args));
                                stack.push(Walk::Tok(&g.open));
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Why a statement is kept as [`Unknown`] although its major word is registered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnknownReason {
    /// The statement does not match any descriptor of its major word (lenient mode only).
    ShapeMismatch,
    /// The statement does not start with a major word.
    NoMajorWord,
}

impl UnknownReason {
    /// Stable snake_case name.
    pub fn as_str(self) -> &'static str {
        match self {
            UnknownReason::ShapeMismatch => "shape_mismatch",
            UnknownReason::NoMajorWord => "no_major_word",
        }
    }
}

/// A statement kept as raw tokens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unknown {
    /// Line and column of the statement's first token.
    pub pos: Pos,
    /// The major word in upper case, if the statement has one.
    pub keyword: Option<Box<str>>,
    /// Every token of the statement in input order, comments included.
    pub tokens: Vec<Token>,
    /// `None` when the major word has no descriptor.
    pub reason: Option<UnknownReason>,
}

/// One node of the tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    /// A statement that matched a descriptor.
    Statement(Box<Statement>),
    /// A statement kept as raw tokens.
    Unknown(Unknown),
    /// A comment line outside any statement.
    Comment(Token),
}

impl Node {
    /// Line and column of the node's first token.
    pub fn pos(&self) -> Pos {
        match self {
            Node::Statement(s) => s.pos,
            Node::Unknown(u) => u.pos,
            Node::Comment(t) => t.pos,
        }
    }

    /// Every token of the node in input order.
    pub fn tokens(&self) -> Vec<&Token> {
        match self {
            Node::Statement(s) => s.tokens(),
            Node::Unknown(u) => u.tokens.iter().collect(),
            Node::Comment(t) => vec![t],
        }
    }
}

/// A parsed DMIS file.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DmisTree {
    /// One node per logical statement, in input order.
    pub statements: Vec<Node>,
}

impl DmisTree {
    /// Every token of the input except whitespace and continuation markers, in input order.
    pub fn tokens(&self) -> Vec<&Token> {
        self.statements.iter().flat_map(Node::tokens).collect()
    }
}

/// Parse options.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ParseOptions {
    /// Keep statements that match no descriptor of their major word as `Unknown` with reason
    /// `shape_mismatch` instead of failing.
    pub lenient: bool,
}

/// Which tree bound was exceeded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TreeBound {
    /// [`crate::registry::MAX_MATCH_STEPS`].
    MatchSteps,
    /// [`MAX_GROUP_DEPTH`].
    GroupDepth,
}

impl TreeBound {
    /// Stable snake_case name.
    pub fn as_str(self) -> &'static str {
        match self {
            TreeBound::MatchSteps => "match_steps",
            TreeBound::GroupDepth => "group_depth",
        }
    }
}

/// A parse failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// The lexer or the reader harness failed.
    Lex(Box<LexError>),
    /// A statement with a registered major word matches none of its descriptors.
    Malformed {
        /// 1-based line where matching stopped.
        line: u32,
        /// 1-based byte column where matching stopped.
        col: u32,
        /// The class that got farthest.
        class: Box<str>,
        /// Position of the statement's first token.
        statement: Pos,
    },
    /// A tree bound was exceeded in the statement starting at `line`.
    LimitExceeded {
        /// Which bound.
        bound: TreeBound,
        /// First line of the statement.
        line: u32,
    },
    /// The registry with the dialect's descriptors could not be built.
    Registry(RegistryError),
}

impl ParseError {
    /// The stable error code: `malformed`, `limit_exceeded`, the lexer's or harness code, or
    /// the registry code.
    pub fn code(&self) -> &'static str {
        match self {
            ParseError::Lex(e) => e.code(),
            ParseError::Malformed { .. } => "malformed",
            ParseError::LimitExceeded { .. } => "limit_exceeded",
            ParseError::Registry(e) => e.code(),
        }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::Lex(e) => e.fmt(f),
            ParseError::Malformed { line, col, class, .. } => {
                write!(f, "malformed: statement does not match class {class} at line {line}, column {col}")
            }
            ParseError::LimitExceeded { bound, line } => {
                write!(f, "limit_exceeded: {} in the statement starting at line {line}", bound.as_str())
            }
            ParseError::Registry(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for ParseError {}

impl From<LexError> for ParseError {
    fn from(e: LexError) -> Self {
        ParseError::Lex(Box::new(e))
    }
}

/// Parses `data` with the built-in registry ([`Registry::builtin`]), plus the dialect's
/// descriptors and lexer options if one is given; strict mode, default script limits.
pub fn parse(data: &[u8], dialect: Option<&Dialect>) -> Result<DmisTree, ParseError> {
    let base = Registry::builtin().map_err(ParseError::Registry)?;
    parse_in(data, &base, dialect, ParseOptions::default())
}

/// Parses `data` with `registry` (plus the dialect, if any) under the default script limits.
pub fn parse_in(
    data: &[u8],
    registry: &Registry,
    dialect: Option<&Dialect>,
    options: ParseOptions,
) -> Result<DmisTree, ParseError> {
    let budget = Budget::new(Principal::Script, &LimitRequest::default()).map_err(LexError::Io)?;
    match dialect {
        None => parse_with(data, registry, &LexOptions::strict(), options, budget),
        Some(d) => {
            let merged = registry.with_dialect(d).map_err(ParseError::Registry)?;
            parse_with(data, &merged, &d.lex, options, budget)
        }
    }
}

/// Parses `data` with `registry` and lexer options `lex` under `budget`.
pub fn parse_with(
    data: &[u8],
    registry: &Registry,
    lex: &LexOptions,
    options: ParseOptions,
    budget: Budget,
) -> Result<DmisTree, ParseError> {
    let stmts = lex_slice(data, budget, lex)?;
    let mut statements = Vec::with_capacity(stmts.len());
    for st in stmts {
        statements.push(build_node(st, registry, options)?);
    }
    Ok(DmisTree { statements })
}

fn is_comment(t: &Token) -> bool {
    t.kind == TokenKind::Comment
}

fn word_upper(t: &Token) -> Option<&str> {
    match &t.kind {
        TokenKind::Word { upper } => Some(upper),
        _ => None,
    }
}

/// Head of a statement: index of the label among the non-comment tokens, and of the major word.
fn head(sig: &[&Token]) -> Option<(Option<usize>, usize)> {
    match sig {
        [l, e, w, ..]
            if matches!(l.kind, TokenKind::Label { .. }) && e.kind == TokenKind::Equals && word_upper(w).is_some() =>
        {
            Some((Some(0), 2))
        }
        [w, ..] if word_upper(w).is_some() => Some((None, 0)),
        _ => None,
    }
}

/// Why the structure could not be built.
enum Shaping {
    /// A shape mismatch at this token.
    Mismatch(Pos),
    /// Groups nested too deeply.
    TooDeep,
}

struct Frame {
    open: Option<Token>,
    args: Vec<Arg>,
    current: Vec<Item>,
}

impl Frame {
    fn new(open: Option<Token>) -> Self {
        Frame { open, args: Vec::new(), current: Vec::new() }
    }

    /// The finished field list: zero fields when nothing was written, else the last field too.
    fn finish(mut self) -> (Option<Token>, Vec<Arg>) {
        if !(self.args.is_empty() && self.current.is_empty()) {
            self.args.push(Arg { items: self.current, comma: None });
        }
        (self.open, self.args)
    }
}

/// Builds the statement skeleton (class not yet known) from the tokens, cloned.
fn shape_statement(tokens: &[Token], label_at: Option<usize>, major_at: usize) -> Result<Statement, Shaping> {
    let mut label_token: Option<Token> = None;
    let mut label: Option<Label> = None;
    let mut major: Option<Token> = None;
    let mut slash: Option<Token> = None;
    let mut trivia = Vec::new();
    let mut stack: Vec<Frame> = Vec::new();
    let mut k = 0usize;
    for t in tokens {
        if is_comment(t) {
            trivia.push(Trivia { before: k, token: t.clone() });
            continue;
        }
        let idx = k;
        k += 1;
        if Some(idx) == label_at {
            label_token = Some(t.clone());
            continue;
        }
        if idx < major_at {
            // The `=` after the label.
            if let Some(token) = label_token.take() {
                label = Some(Label { token, equals: t.clone() });
            }
            continue;
        }
        if idx == major_at {
            major = Some(t.clone());
            continue;
        }
        if slash.is_none() {
            if t.kind != TokenKind::Slash {
                return Err(Shaping::Mismatch(t.pos));
            }
            slash = Some(t.clone());
            stack.push(Frame::new(None));
            continue;
        }
        let Some(top) = stack.last_mut() else { return Err(Shaping::Mismatch(t.pos)) };
        match t.kind {
            TokenKind::Comma => {
                let items = std::mem::take(&mut top.current);
                top.args.push(Arg { items, comma: Some(t.clone()) });
            }
            TokenKind::LParen => {
                // The top-level frame does not count as a group.
                if stack.len() > MAX_GROUP_DEPTH {
                    return Err(Shaping::TooDeep);
                }
                stack.push(Frame::new(Some(t.clone())));
            }
            TokenKind::RParen => {
                if stack.len() < 2 {
                    return Err(Shaping::Mismatch(t.pos));
                }
                let Some(frame) = stack.pop() else { return Err(Shaping::Mismatch(t.pos)) };
                let (open, args) = frame.finish();
                let Some(open) = open else { return Err(Shaping::Mismatch(t.pos)) };
                let Some(parent) = stack.last_mut() else { return Err(Shaping::Mismatch(t.pos)) };
                parent.current.push(Item::Group(Group { open, args, close: t.clone() }));
            }
            _ => top.current.push(Item::Atom(t.clone())),
        }
    }
    let Some(major) = major else {
        let at = tokens.first().map_or(Pos { line: 1, col: 1 }, |t| t.pos);
        return Err(Shaping::Mismatch(at));
    };
    let mut args = Vec::new();
    if let Some(frame) = stack.pop() {
        if let Some(open) = &frame.open {
            // The innermost group still open at the end.
            return Err(Shaping::Mismatch(open.pos));
        }
        args = frame.finish().1;
    }
    let pos = tokens.first().map_or(major.pos, |t| t.pos);
    Ok(Statement { pos, label, major, slash, args, class: Box::from(""), trivia })
}

/// Position of field `i` for an error report: its first token, else its comma, else `last`.
fn field_pos(args: &[Arg], i: usize, last: Pos) -> Pos {
    match args.get(i) {
        Some(arg) => match (arg.items.first(), &arg.comma) {
            (Some(item), _) => item.pos(),
            (None, Some(comma)) => comma.pos,
            (None, None) => last,
        },
        None => last,
    }
}

fn unknown(tokens: Vec<Token>, keyword: Option<&str>, reason: Option<UnknownReason>) -> Node {
    let pos = tokens.first().map_or(Pos { line: 0, col: 0 }, |t| t.pos);
    Node::Unknown(Unknown { pos, keyword: keyword.map(Box::from), tokens, reason })
}

fn build_node(st: LogicalStatement, registry: &Registry, options: ParseOptions) -> Result<Node, ParseError> {
    let LogicalStatement { tokens, first_line, .. } = st;
    if let [only] = tokens.as_slice() {
        if is_comment(only) {
            return Ok(Node::Comment(only.clone()));
        }
    }
    let (found, last) = {
        let sig: Vec<&Token> = tokens.iter().filter(|t| !is_comment(t)).collect();
        let found = head(&sig).map(|(label_at, major_at)| {
            let major = sig.get(major_at).and_then(|t| word_upper(t)).unwrap_or("").to_string();
            (label_at, major_at, major)
        });
        (found, sig.last().map_or(Pos { line: first_line, col: 1 }, |t| t.pos))
    };
    let Some((label_at, major_at, major)) = found else {
        return Ok(unknown(tokens, None, Some(UnknownReason::NoMajorWord)));
    };
    if !registry.knows(&major) {
        return Ok(unknown(tokens, Some(&major), None));
    }
    let first_class = || registry.for_major(&major).first().map_or(Box::from(""), |d| d.class.clone());
    let statement_pos = tokens.first().map_or(Pos { line: first_line, col: 1 }, |t| t.pos);
    let mismatch = |tokens: Vec<Token>, at: Pos, class: Box<str>| {
        if options.lenient {
            Ok(unknown(tokens, Some(&major), Some(UnknownReason::ShapeMismatch)))
        } else {
            Err(ParseError::Malformed { line: at.line, col: at.col, class, statement: statement_pos })
        }
    };
    let mut skeleton = match shape_statement(&tokens, label_at, major_at) {
        Ok(s) => s,
        Err(Shaping::TooDeep) => {
            return Err(ParseError::LimitExceeded { bound: TreeBound::GroupDepth, line: first_line })
        }
        Err(Shaping::Mismatch(at)) => return mismatch(tokens, at, first_class()),
    };
    let label_prefix = skeleton.label.as_ref().map(|l| l.prefix().to_string());
    let attempts = registry
        .attempt(&major, label_prefix.as_deref(), &skeleton.args)
        .map_err(|_| ParseError::LimitExceeded { bound: TreeBound::MatchSteps, line: first_line })?;
    if let Some(hit) = attempts.iter().find(|a| matches!(a.result, Some((true, _)))) {
        skeleton.class = hit.desc.class.clone();
        return Ok(Node::Statement(Box::new(skeleton)));
    }
    // The descriptor that got farthest; the first one on a tie.
    let mut best: Option<(Option<usize>, &StatementDescriptor)> = None;
    for a in &attempts {
        let reach = a.result.map(|(_, far)| far);
        let better = match &best {
            None => true,
            Some((r, _)) => reach > *r,
        };
        if better {
            best = Some((reach, a.desc));
        }
    }
    let (at, class) = match best {
        Some((Some(far), d)) => (field_pos(&skeleton.args, far, last), d.class.clone()),
        Some((None, d)) => (statement_pos, d.class.clone()),
        None => (statement_pos, first_class()),
    };
    mismatch(tokens, at, class)
}
