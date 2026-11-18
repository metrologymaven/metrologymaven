//! Printer: the shortest source for a tree that parses back to the same tree.
//!
//! Parentheses appear only where precedence or associativity needs them, and a space only
//! where two tokens would otherwise merge: a name or keyword followed by a word character,
//! or a number literal followed by a digit or by `[` (which would lex as a quantity). A
//! number followed by a letter needs no space (`1and x` lexes as `1`, `and`, `x`), so the
//! output favours shortness over looks. Integers print without leading
//! zeros and strings use the shortest escapes, so the output is never longer, never has
//! more tokens and never nests deeper than any source of the same tree; print-then-parse
//! therefore stays within the parse bounds. Uses an explicit stack, so any depth is safe.

use super::{is_hidden_char, Expr, ExprKind, UnaryOp, NOT_PRECEDENCE};

/// Precedence of the node's top level: 0 conditional, 1, 2 and 4 to 6 binary (see
/// [`super::BinaryOp::precedence`]), 3 `not`, 7 unary minus, 8 postfix and primary.
fn prec(e: &Expr) -> u8 {
    match &e.kind {
        ExprKind::Cond(..) => 0,
        ExprKind::Binary(op, ..) => op.precedence(),
        ExprKind::Unary(UnaryOp::Not, _) => NOT_PRECEDENCE,
        ExprKind::Unary(UnaryOp::Neg, _) => 7,
        _ => 8,
    }
}

enum W<'e> {
    E(&'e Expr),
    T(&'static str),
    S(String),
    /// Integer or decimal literal text.
    Num(String),
}

fn operand<'e>(stack: &mut Vec<W<'e>>, e: &'e Expr, paren: bool) {
    if paren {
        stack.push(W::T(")"));
        stack.push(W::E(e));
        stack.push(W::T("("));
    } else {
        stack.push(W::E(e));
    }
}

/// String literal with the shortest escapes: `\"`, `\\`, `\n`, `\r`, `\t`, and `\u{h}` for
/// the other characters [`is_hidden_char`] refuses raw.
fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if is_hidden_char(c) => out.push_str(&format!("\\u{{{:x}}}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

struct Out {
    text: String,
    last_numeric: bool,
}

impl Out {
    fn piece(&mut self, s: &str, numeric: bool) {
        if let (Some(prev), Some(next)) = (self.text.chars().next_back(), s.chars().next()) {
            let merge =
                if self.last_numeric { next.is_ascii_digit() || next == '[' } else { is_word(prev) && is_word(next) };
            if merge {
                self.text.push(' ');
            }
        }
        self.text.push_str(s);
        self.last_numeric = numeric;
    }
}

/// Print an expression as source text (REQ-AUTO.PROG.EXPR.GRAMMAR-006). For every tree
/// [`super::parse_expr`] returns, parsing the printed text gives an equal tree (spans aside).
pub fn print_expr(expr: &Expr) -> String {
    let mut out = Out { text: String::new(), last_numeric: false };
    let mut stack: Vec<W<'_>> = vec![W::E(expr)];
    while let Some(w) = stack.pop() {
        let e = match w {
            W::T(s) => {
                out.piece(s, false);
                continue;
            }
            W::S(s) => {
                out.piece(&s, false);
                continue;
            }
            W::Num(s) => {
                out.piece(&s, true);
                continue;
            }
            W::E(e) => e,
        };
        match &e.kind {
            ExprKind::Int(v) => stack.push(W::Num(v.to_string())),
            ExprKind::Dec(d) => stack.push(W::Num(d.as_str().to_string())),
            ExprKind::Qty(d, u) => stack.push(W::S(format!("{}[{}]", d.as_str(), u.as_str()))),
            ExprKind::Str(s) => stack.push(W::S(quote(s))),
            ExprKind::Bool(b) => stack.push(W::T(if *b { "true" } else { "false" })),
            ExprKind::Ident(n) => stack.push(W::S(n.as_str().to_string())),
            ExprKind::Field(base, name) => {
                stack.push(W::S(format!(".{}", name.as_str())));
                operand(&mut stack, base, prec(base) < 8);
            }
            ExprKind::Index(base, ix) => {
                stack.push(W::T("]"));
                stack.push(W::E(ix));
                stack.push(W::T("["));
                operand(&mut stack, base, prec(base) < 8);
            }
            ExprKind::Unary(op, arg) => {
                // `-` takes a unary operand; `not` an operand at its own level or tighter.
                let level = if *op == UnaryOp::Neg { 7 } else { NOT_PRECEDENCE };
                operand(&mut stack, arg, prec(arg) < level);
                stack.push(W::T(op.as_str()));
            }
            ExprKind::Binary(op, l, r) => {
                let bp = op.precedence();
                operand(&mut stack, r, prec(r) <= bp);
                stack.push(W::T(op.as_str()));
                operand(&mut stack, l, prec(l) < bp);
            }
            ExprKind::Cond(c, a, b) => {
                stack.push(W::E(b));
                stack.push(W::T("else"));
                stack.push(W::E(a));
                stack.push(W::T("then"));
                stack.push(W::E(c));
                stack.push(W::T("if"));
            }
            ExprKind::Call(name, args) => {
                stack.push(W::T(")"));
                for (k, a) in args.iter().enumerate().rev() {
                    stack.push(W::E(a));
                    if k > 0 {
                        stack.push(W::T(","));
                    }
                }
                stack.push(W::S(format!("{}(", name.as_str())));
            }
        }
    }
    out.text
}
