//! Pratt parser with an explicit depth counter.
//!
//! Depth is measured as in [`MAX_DEPTH`]: every AST edge and every pair of parentheses is
//! one level. Each parsing function receives `depth`, the number of levels above the
//! position it fills, and refuses to descend past `MAX_DEPTH`, so recursion is bounded by a
//! small constant times `MAX_DEPTH`. Each function also returns the height of what it
//! built; a node whose `depth + height` exceeds the bound (a long left-associative chain,
//! for example) is refused as soon as it is built.

use super::lex::{Kw, Tok, Token, P};
use super::{syntax, BinaryOp, DecText, Expr, ExprKind, Limit, Name, ParseError, Span, UnaryOp, UnitSym, MAX_DEPTH};

type Built = Result<(Expr, usize), ParseError>;

pub(super) fn parse(tokens: &[Token], src_len: usize) -> Result<Expr, ParseError> {
    let mut p = Parser { toks: tokens, pos: 0, eof: src_len };
    let (e, _) = p.expr(0)?;
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

fn too_deep() -> ParseError {
    ParseError::TooLarge(Limit::Depth)
}

/// Refuse a node at `depth` whose subtree has height `h`.
fn bounded(depth: usize, h: usize) -> Result<usize, ParseError> {
    if depth.saturating_add(h) > MAX_DEPTH {
        Err(too_deep())
    } else {
        Ok(h)
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
        if depth > MAX_DEPTH {
            return Err(too_deep());
        }
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
        let (c, hc) = self.expr(depth + 1)?;
        self.expect(&Tok::Kw(Kw::Then))?;
        let (a, ha) = self.expr(depth + 1)?;
        self.expect(&Tok::Kw(Kw::Else))?;
        let (b, hb) = self.expr(depth + 1)?;
        let h = bounded(depth, 1 + hc.max(ha).max(hb))?;
        let span = Span::new(start, b.span.end);
        Ok((Expr::new(ExprKind::Cond(boxed(c), boxed(a), boxed(b)), span), h))
    }

    /// Operators binding tighter than `min`, left-associative (Pratt loop).
    fn binary(&mut self, min: u8, depth: usize) -> Built {
        let (mut lhs, mut lh) = self.unary(depth)?;
        while let Some(op) = self.peek_tok().and_then(binary_op) {
            let bp = op.precedence();
            if bp <= min {
                break;
            }
            self.pos += 1;
            let (rhs, rh) = self.binary(bp, depth + 1)?;
            lh = bounded(depth, 1 + lh.max(rh))?;
            let span = Span::new(lhs.span.start, rhs.span.end);
            lhs = Expr::new(ExprKind::Binary(op, boxed(lhs), boxed(rhs)), span);
        }
        Ok((lhs, lh))
    }

    /// unary = ("-" | "not") unary | postfix
    fn unary(&mut self, depth: usize) -> Built {
        if depth > MAX_DEPTH {
            return Err(too_deep());
        }
        let op = match self.peek_tok() {
            Some(Tok::P(P::Minus)) => UnaryOp::Neg,
            Some(Tok::Kw(Kw::Not)) => UnaryOp::Not,
            _ => return self.postfix(depth),
        };
        let start = self.here();
        self.pos += 1;
        let (arg, h) = self.unary(depth + 1)?;
        let h = bounded(depth, h + 1)?;
        let span = Span::new(start, arg.span.end);
        Ok((Expr::new(ExprKind::Unary(op, boxed(arg)), span), h))
    }

    /// postfix = primary { "." name | "[" expr "]" }
    fn postfix(&mut self, depth: usize) -> Built {
        let (mut e, mut h) = self.primary(depth)?;
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
                    h = bounded(depth, h + 1)?;
                    let span = Span::new(e.span.start, t.end);
                    e = Expr::new(ExprKind::Field(boxed(e), name), span);
                }
                Some(Tok::P(P::LBracket)) => {
                    self.pos += 1;
                    let (ix, ih) = self.expr(depth + 1)?;
                    let end = self.expect(&Tok::P(P::RBracket))?;
                    h = bounded(depth, 1 + h.max(ih))?;
                    let span = Span::new(e.span.start, end);
                    e = Expr::new(ExprKind::Index(boxed(e), boxed(ix)), span);
                }
                _ => return Ok((e, h)),
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
        let leaf = |kind: ExprKind| -> Built { Ok((Expr::new(kind, span), 0)) };
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
                let (mut e, h) = self.expr(depth + 1)?;
                let end = self.expect(&Tok::P(P::RParen))?;
                let h = bounded(depth, h + 1)?;
                e.span = Span::new(at, end);
                Ok((e, h))
            }
            _ => Err(syntax(at)),
        }
    }

    /// Arguments after `name(`: `[ expr { "," expr } ] ")"`.
    fn call_rest(&mut self, name: Name, start: usize, depth: usize) -> Built {
        let mut args = Vec::new();
        let mut h = 0usize;
        if let Some(end) = self.eat_rparen() {
            return Ok((Expr::new(ExprKind::Call(name, args), Span::new(start, end)), 0));
        }
        loop {
            let (a, ah) = self.expr(depth + 1)?;
            h = h.max(ah + 1);
            args.push(a);
            if self.peek_tok() == Some(&Tok::P(P::Comma)) {
                self.pos += 1;
                continue;
            }
            let end = self.expect(&Tok::P(P::RParen))?;
            let h = bounded(depth, h)?;
            return Ok((Expr::new(ExprKind::Call(name, args), Span::new(start, end)), h));
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
