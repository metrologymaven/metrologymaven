//! Pratt parser with an explicit depth counter.
//!
//! Depth is measured as in [`MAX_DEPTH`]: only nesting counts (a pair of parentheses, an
//! index, a call's arguments, a unary operator, a conditional's parts). Each parsing
//! function receives `depth`, the nesting level of the position it fills, and refuses to
//! go past `MAX_DEPTH`. Left-associative binary operators and `.field` chains are built in
//! a loop, so a flat chain such as `a+b+...+z` adds no depth and no recursion; the
//! right-hand operand of a binary operator recurses only into tighter-binding levels, of
//! which there are at most five. Recursion is therefore bounded by a small constant times
//! `MAX_DEPTH`.

use super::lex::{Kw, Tok, Token, P};
use super::{
    syntax, BinaryOp, DecText, Expr, ExprKind, Limit, Name, ParseError, Span, UnaryOp, UnitSym, MAX_DEPTH,
    NOT_PRECEDENCE,
};

type Built = Result<Expr, ParseError>;

pub(super) fn parse(tokens: &[Token], src_len: usize) -> Result<Expr, ParseError> {
    let mut p = Parser { toks: tokens, pos: 0, eof: src_len };
    let e = p.expr(0)?;
    match p.peek() {
        None => Ok(e),
        Some(t) => Err(syntax(t.start)),
    }
}

struct Parser<'a> {
    toks: &'a [Token],
    pos: usize,
    eof: usize,
}

/// Refuse to fill a position nested `depth` levels deep.
fn enter(depth: usize) -> Result<(), ParseError> {
    if depth > MAX_DEPTH {
        Err(ParseError::TooLarge(Limit::Depth))
    } else {
        Ok(())
    }
}

fn binary_op(t: &Tok) -> Option<BinaryOp> {
    Some(match t {
        Tok::P(P::Star) => BinaryOp::Mul,
        Tok::P(P::Slash) => BinaryOp::Div,
        Tok::P(P::Percent) => BinaryOp::Rem,
        Tok::P(P::Plus) => BinaryOp::Add,
        Tok::P(P::Minus) => BinaryOp::Sub,
        Tok::P(P::Lt) => BinaryOp::Lt,
        Tok::P(P::Le) => BinaryOp::Le,
        Tok::P(P::Gt) => BinaryOp::Gt,
        Tok::P(P::Ge) => BinaryOp::Ge,
        Tok::P(P::EqEq) => BinaryOp::Eq,
        Tok::P(P::Ne) => BinaryOp::Ne,
        Tok::Kw(Kw::And) => BinaryOp::And,
        Tok::Kw(Kw::Or) => BinaryOp::Or,
        _ => return None,
    })
}

fn boxed(e: Expr) -> Box<Expr> {
    Box::new(e)
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<&'a Token> {
        self.toks.get(self.pos)
    }

    fn peek_tok(&self) -> Option<&'a Tok> {
        self.peek().map(|t| &t.tok)
    }

    /// Offset of the current token, or the source length at the end.
    fn here(&self) -> usize {
        self.peek().map_or(self.eof, |t| t.start)
    }

    fn bump(&mut self) -> Option<&'a Token> {
        let t = self.toks.get(self.pos);
        if t.is_some() {
            self.pos += 1;
        }
        t
    }

    /// Consume `want` or fail at the current token. Returns the consumed token's end.
    fn expect(&mut self, want: &Tok) -> Result<usize, ParseError> {
        match self.peek() {
            Some(t) if &t.tok == want => {
                self.pos += 1;
                Ok(t.end)
            }
            _ => Err(syntax(self.here())),
        }
    }

    /// expr = cond | or-expr
    fn expr(&mut self, depth: usize) -> Built {
        enter(depth)?;
        if self.peek_tok() == Some(&Tok::Kw(Kw::If)) {
            self.cond(depth)
        } else {
            self.binary(0, depth)
        }
    }

    /// cond = "if" expr "then" expr "else" expr
    fn cond(&mut self, depth: usize) -> Built {
        let start = self.here();
        self.pos += 1;
        let c = self.expr(depth + 1)?;
        self.expect(&Tok::Kw(Kw::Then))?;
        let a = self.expr(depth + 1)?;
        self.expect(&Tok::Kw(Kw::Else))?;
        let b = self.expr(depth + 1)?;
        let span = Span::new(start, b.span.end);
        Ok(Expr::new(ExprKind::Cond(boxed(c), boxed(a), boxed(b)), span))
    }

    /// Operators binding tighter than `min`, left-associative (Pratt loop). A `not` prefix
    /// is allowed where `min` is below the comparison level: at the top, after `and` or
    /// `or`, and after another `not`.
    fn binary(&mut self, min: u8, depth: usize) -> Built {
        let mut lhs = if self.peek_tok() == Some(&Tok::Kw(Kw::Not)) && min <= NOT_PRECEDENCE {
            enter(depth + 1)?;
            let start = self.here();
            self.pos += 1;
            let arg = self.binary(NOT_PRECEDENCE, depth + 1)?;
            let span = Span::new(start, arg.span.end);
            Expr::new(ExprKind::Unary(UnaryOp::Not, boxed(arg)), span)
        } else {
            self.unary(depth)?
        };
        while let Some(op) = self.peek_tok().and_then(binary_op) {
            let bp = op.precedence();
            if bp <= min {
                break;
            }
            self.pos += 1;
            let rhs = self.binary(bp, depth)?;
            let span = Span::new(lhs.span.start, rhs.span.end);
            lhs = Expr::new(ExprKind::Binary(op, boxed(lhs), boxed(rhs)), span);
        }
        Ok(lhs)
    }

    /// unary = "-" unary | postfix
    fn unary(&mut self, depth: usize) -> Built {
        enter(depth)?;
        if self.peek_tok() != Some(&Tok::P(P::Minus)) {
            return self.postfix(depth);
        }
        let start = self.here();
        self.pos += 1;
        let arg = self.unary(depth + 1)?;
        let span = Span::new(start, arg.span.end);
        Ok(Expr::new(ExprKind::Unary(UnaryOp::Neg, boxed(arg)), span))
    }

    /// postfix = primary { "." name | "[" expr "]" }
    fn postfix(&mut self, depth: usize) -> Built {
        let mut e = self.primary(depth)?;
        loop {
            match self.peek_tok() {
                Some(Tok::P(P::Dot)) => {
                    self.pos += 1;
                    let t = self.peek();
                    let name = match t.map(|t| &t.tok) {
                        Some(Tok::Ident(n)) => Name::new(n),
                        _ => None,
                    };
                    let (Some(t), Some(name)) = (t, name) else {
                        return Err(syntax(self.here()));
                    };
                    self.pos += 1;
                    let span = Span::new(e.span.start, t.end);
                    e = Expr::new(ExprKind::Field(boxed(e), name), span);
                }
                Some(Tok::P(P::LBracket)) => {
                    self.pos += 1;
                    let ix = self.expr(depth + 1)?;
                    let end = self.expect(&Tok::P(P::RBracket))?;
                    let span = Span::new(e.span.start, end);
                    e = Expr::new(ExprKind::Index(boxed(e), boxed(ix)), span);
                }
                _ => return Ok(e),
            }
        }
    }

    /// primary = literal | name | call | "(" expr ")"
    fn primary(&mut self, depth: usize) -> Built {
        let at = self.here();
        let Some(t) = self.bump() else {
            return Err(syntax(at));
        };
        let span = Span::new(t.start, t.end);
        let leaf = |kind: ExprKind| -> Built { Ok(Expr::new(kind, span)) };
        match &t.tok {
            Tok::Int(v) => leaf(ExprKind::Int(*v)),
            Tok::Dec(d) => match DecText::new(d) {
                Some(d) => leaf(ExprKind::Dec(d)),
                None => Err(syntax(at)),
            },
            Tok::Qty(d, u) => match (DecText::new(d), UnitSym::new(u)) {
                (Some(d), Some(u)) => leaf(ExprKind::Qty(d, u)),
                _ => Err(syntax(at)),
            },
            Tok::Str(s) => leaf(ExprKind::Str(s.clone())),
            Tok::Kw(Kw::True) => leaf(ExprKind::Bool(true)),
            Tok::Kw(Kw::False) => leaf(ExprKind::Bool(false)),
            Tok::Ident(n) => {
                let Some(name) = Name::new(n) else {
                    return Err(syntax(at));
                };
                if self.peek_tok() == Some(&Tok::P(P::LParen)) {
                    self.pos += 1;
                    self.call_rest(name, at, depth)
                } else {
                    leaf(ExprKind::Ident(name))
                }
            }
            Tok::P(P::LParen) => {
                let mut e = self.expr(depth + 1)?;
                let end = self.expect(&Tok::P(P::RParen))?;
                e.span = Span::new(at, end);
                Ok(e)
            }
            _ => Err(syntax(at)),
        }
    }

    /// Arguments after `name(`: `[ expr { "," expr } ] ")"`.
    fn call_rest(&mut self, name: Name, start: usize, depth: usize) -> Built {
        let mut args = Vec::new();
        if let Some(end) = self.eat_rparen() {
            return Ok(Expr::new(ExprKind::Call(name, args), Span::new(start, end)));
        }
        loop {
            args.push(self.expr(depth + 1)?);
            if self.peek_tok() == Some(&Tok::P(P::Comma)) {
                self.pos += 1;
                continue;
            }
            let end = self.expect(&Tok::P(P::RParen))?;
            return Ok(Expr::new(ExprKind::Call(name, args), Span::new(start, end)));
        }
    }

    fn eat_rparen(&mut self) -> Option<usize> {
        match self.peek() {
            Some(t) if t.tok == Tok::P(P::RParen) => {
                self.pos += 1;
                Some(t.end)
            }
            _ => None,
        }
    }
}
