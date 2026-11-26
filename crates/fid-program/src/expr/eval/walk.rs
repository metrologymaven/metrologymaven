//! The tree walk: an explicit work stack and value stack, so tree depth never meets the
//! call stack.

use super::dec::{Dec, DecFail};
use super::func;
use super::num::{cmp_real_dec, cmp_real_plain, dec_to_f64, finite, unit_kind, Fail};
use super::{Env, EvalError, EvalKind, Value, V};
use crate::expr::syntax::{BinaryOp, Expr, ExprKind, UnaryOp};
use crate::expr::types::infer::Kinds;
use crate::expr::types::{type_of, Dim, Kind, Type};
use fid_registry::QuantityCatalogue;
use std::cmp::Ordering;
use std::collections::BTreeMap;

/// What one evaluation reads: the catalogue, its kinds, the bindings and their types.
pub(crate) struct Ctx<'a> {
    pub(crate) cat: &'a dyn QuantityCatalogue,
    pub(crate) kinds: &'a Kinds,
    pub(crate) env: &'a Env,
    pub(crate) types: &'a BTreeMap<String, Type>,
}

enum Work<'e> {
    Visit(&'e Expr),
    Apply(&'e Expr),
    /// The left operand of `and`/`or` is on the value stack.
    Logic(&'e Expr),
    /// The condition of an `if` is on the value stack.
    Pick(&'e Expr),
    /// The chosen branch of this `if` is on the value stack.
    Picked(&'e Expr),
}

fn operands(e: &Expr) -> Vec<&Expr> {
    match &e.kind {
        ExprKind::Field(b, _) => vec![&**b],
        ExprKind::Index(b, i) => vec![&**b, &**i],
        ExprKind::Unary(_, x) => vec![&**x],
        ExprKind::Binary(_, l, r) => vec![&**l, &**r],
        ExprKind::Call(_, args) => args.iter().collect(),
        _ => Vec::new(),
    }
}

fn dec_fail(f: DecFail) -> Fail {
    match f {
        DecFail::Overflow => Fail::DecOverflow,
        DecFail::DivZero => Fail::Kind(EvalKind::DivZero),
        DecFail::Syntax => Fail::Internal,
    }
}

fn at(e: &Expr, f: Fail) -> EvalError {
    match f {
        Fail::Kind(kind) => EvalError::Eval { kind, span: e.span },
        Fail::DecOverflow => EvalError::DecimalOverflow { span: e.span },
        Fail::Internal => EvalError::Internal { span: e.span },
    }
}

/// Evaluates `root` with an explicit stack.
pub(crate) fn run(ctx: &Ctx<'_>, root: &Expr) -> Result<Value, EvalError> {
    let mut work = vec![Work::Visit(root)];
    let mut vals: Vec<Value> = Vec::new();
    while let Some(w) = work.pop() {
        match w {
            Work::Visit(e) => match &e.kind {
                ExprKind::Binary(BinaryOp::And | BinaryOp::Or, l, _) => {
                    work.push(Work::Logic(e));
                    work.push(Work::Visit(l));
                }
                ExprKind::Cond(c, _, _) => {
                    work.push(Work::Pick(e));
                    work.push(Work::Visit(c));
                }
                _ => {
                    work.push(Work::Apply(e));
                    work.extend(operands(e).into_iter().rev().map(Work::Visit));
                }
            },
            Work::Apply(e) => {
                let n = operands(e).len();
                if vals.len() < n {
                    return Err(EvalError::Internal { span: e.span });
                }
                let args = vals.split_off(vals.len() - n);
                let v = ctx.node(e, args).map_err(|f| at(e, f))?;
                vals.push(v);
            }
            Work::Logic(e) => {
                let l = vals.pop().ok_or(EvalError::Internal { span: e.span })?;
                let (ExprKind::Binary(op, _, r), V::Bool(b)) = (&e.kind, l.v()) else {
                    return Err(EvalError::Internal { span: e.span });
                };
                match (op, *b) {
                    (BinaryOp::And, false) | (BinaryOp::Or, true) => vals.push(Value::bool(*b)),
                    _ => work.push(Work::Visit(r)),
                }
            }
            Work::Pick(e) => {
                let c = vals.pop().ok_or(EvalError::Internal { span: e.span })?;
                let (ExprKind::Cond(_, a, b), V::Bool(t)) = (&e.kind, c.v()) else {
                    return Err(EvalError::Internal { span: e.span });
                };
                work.push(Work::Picked(e));
                work.push(Work::Visit(if *t { a } else { b }));
            }
            Work::Picked(e) => {
                // An `int` branch of an `if` typed `decimal` becomes an exact decimal.
                if let Some(V::Int(i)) = vals.last().map(Value::v) {
                    let i = *i;
                    let (ty, _) = type_of(e, ctx.types, ctx.cat);
                    if ty == Some(Type::Decimal) {
                        vals.pop();
                        vals.push(Value::of_dec(Dec::from_i64(i), None));
                    }
                }
            }
        }
    }
    match (vals.pop(), vals.is_empty()) {
        (Some(v), true) => Ok(v),
        _ => Err(EvalError::Internal { span: root.span }),
    }
}

/// A numeric operand.
#[derive(Clone, Copy)]
pub(crate) enum Num<'a> {
    Int(i64),
    Dec(&'a Dec, Option<&'a str>),
    Real(f64, Option<&'a Kind>),
}

pub(crate) fn num(v: &Value) -> Option<Num<'_>> {
    match v.v() {
        V::Int(i) => Some(Num::Int(*i)),
        V::Dec(d, u) => Some(Num::Dec(d, u.as_deref())),
        V::Real(x, k) => Some(Num::Real(*x, k.as_ref())),
        _ => None,
    }
}

/// An exact operand: an `int` as a decimal, or a decimal with its unit.
fn exact(n: Num<'_>) -> Option<(Dec, Option<&str>)> {
    match n {
        Num::Int(i) => Some((Dec::from_i64(i), None)),
        Num::Dec(d, u) => Some((*d, u)),
        Num::Real(..) => None,
    }
}

impl Ctx<'_> {
    pub(crate) fn kind_of(&self, n: Num<'_>) -> Result<Option<Kind>, Fail> {
        match n {
            Num::Int(_) | Num::Dec(_, None) => Ok(None),
            Num::Dec(_, Some(u)) => unit_kind(self.kinds, u).map(Some),
            Num::Real(_, k) => Ok(k.cloned()),
        }
    }

    pub(crate) fn to_f64(&self, n: Num<'_>) -> Result<f64, Fail> {
        match n {
            Num::Int(i) => Ok(i as f64),
            Num::Dec(d, u) => dec_to_f64(d, u, self.kinds, self.cat),
            Num::Real(x, _) => Ok(x),
        }
    }

    /// The value kind for a dimension: `None` for a plain number, else the type's kind.
    pub(crate) fn dim_kind(&self, d: Dim) -> Result<Option<Kind>, Fail> {
        match self.kinds.type_of_dim(d) {
            Type::Decimal => Ok(None),
            Type::Quantity(k) => Ok(Some(k)),
            _ => Err(Fail::Internal),
        }
    }

    fn dim(&self, k: &Kind) -> Result<Dim, Fail> {
        self.kinds.dim_of(k).ok_or(Fail::Internal)
    }

    /// The kind of `a * b` (`sign` 1) or `a / b` (`sign` -1), by the rules of
    /// `AUTO.PROG.EXPR.TYPES`: a quantity scaled by a number keeps its kind.
    fn product_kind(&self, a: Option<Kind>, b: Option<Kind>, sign: i8) -> Result<Option<Kind>, Fail> {
        match (a, b) {
            (None, None) => Ok(None),
            (Some(k), None) => Ok(Some(k)),
            (None, Some(k)) if sign > 0 => Ok(Some(k)),
            (a, Some(k)) => {
                let da = match a {
                    Some(a) => self.dim(&a)?,
                    None => Dim::NONE,
                };
                let d = da.combine(self.dim(&k)?, sign).ok_or(Fail::Internal)?;
                self.dim_kind(d)
            }
        }
    }

    pub(crate) fn real(&self, x: f64, kind: Option<Kind>) -> Result<Value, Fail> {
        Value::real(finite(x)?, kind).map_err(|_| Fail::Kind(EvalKind::NonFinite))
    }

    fn node(&self, e: &Expr, args: Vec<Value>) -> Result<Value, Fail> {
        match (&e.kind, args.as_slice()) {
            (ExprKind::Int(i), []) => Ok(Value::int(*i)),
            (ExprKind::Dec(t), []) => {
                Dec::from_text(t.as_str(), false).map(|d| Value::of_dec(d, None)).map_err(dec_fail)
            }
            (ExprKind::Qty(t, u), []) => {
                unit_kind(self.kinds, u.as_str())?;
                let d = Dec::from_text(t.as_str(), false).map_err(dec_fail)?;
                Ok(Value::of_dec(d, Some(u.as_str().to_string())))
            }
            (ExprKind::Str(s), []) => Ok(Value::str(s)),
            (ExprKind::Bool(b), []) => Ok(Value::bool(*b)),
            (ExprKind::Ident(n), []) => self.env.get(n.as_str()).cloned().ok_or(Fail::Internal),
            (ExprKind::Field(_, name), [base]) => match base.v() {
                V::Record(m) => m.get(name.as_str()).cloned().ok_or(Fail::Internal),
                _ => Err(Fail::Internal),
            },
            (ExprKind::Index(..), [base, idx]) => match (base.v(), idx.v()) {
                (V::List(xs), V::Int(i)) => {
                    usize::try_from(*i).ok().and_then(|i| xs.get(i)).cloned().ok_or(Fail::Kind(EvalKind::IndexRange))
                }
                _ => Err(Fail::Internal),
            },
            (ExprKind::Unary(UnaryOp::Not, _), [x]) => match x.v() {
                V::Bool(b) => Ok(Value::bool(!b)),
                _ => Err(Fail::Internal),
            },
            (ExprKind::Unary(UnaryOp::Neg, _), [x]) => match num(x).ok_or(Fail::Internal)? {
                Num::Int(i) => i.checked_neg().map(Value::int).ok_or(Fail::Kind(EvalKind::IntOverflow)),
                Num::Dec(d, u) => Ok(Value::of_dec(d.neg(), u.map(str::to_string))),
                Num::Real(r, k) => self.real(-r, k.cloned()),
            },
            (ExprKind::Binary(op, _, _), [l, r]) => self.binary(*op, l, r),
            (ExprKind::Call(name, _), a) => func::call(self, name.as_str(), a),
            _ => Err(Fail::Internal),
        }
    }

    fn binary(&self, op: BinaryOp, l: &Value, r: &Value) -> Result<Value, Fail> {
        use BinaryOp as B;
        match op {
            B::Add | B::Sub | B::Mul | B::Div | B::Rem => {
                let (a, b) = (num(l).ok_or(Fail::Internal)?, num(r).ok_or(Fail::Internal)?);
                self.arith(op, a, b)
            }
            B::Lt | B::Le | B::Gt | B::Ge => {
                let o = self.cmp(l, r)?;
                Ok(Value::bool(match op {
                    B::Lt => o == Ordering::Less,
                    B::Le => o != Ordering::Greater,
                    B::Gt => o == Ordering::Greater,
                    _ => o != Ordering::Less,
                }))
            }
            B::Eq => self.equal(l, r).map(Value::bool),
            B::Ne => self.equal(l, r).map(|b| Value::bool(!b)),
            // Short-circuited by the walker; never applied.
            B::And | B::Or => Err(Fail::Internal),
        }
    }

    fn arith(&self, op: BinaryOp, a: Num<'_>, b: Num<'_>) -> Result<Value, Fail> {
        use BinaryOp as B;
        if let (Num::Int(x), Num::Int(y)) = (a, b) {
            let ov = || Fail::Kind(EvalKind::IntOverflow);
            return match op {
                B::Add => x.checked_add(y).map(Value::int).ok_or_else(ov),
                B::Sub => x.checked_sub(y).map(Value::int).ok_or_else(ov),
                B::Mul => x.checked_mul(y).map(Value::int).ok_or_else(ov),
                B::Rem if y == 0 => Err(EvalKind::DivZero.into()),
                B::Rem => x.checked_rem(y).map(Value::int).ok_or_else(ov),
                B::Div if y == 0 => Err(EvalKind::DivZero.into()),
                _ => self.real(x as f64 / y as f64, None),
            };
        }
        if let (Some((x, ux)), Some((y, uy))) = (exact(a), exact(b)) {
            let unit = match (op, ux, uy) {
                (B::Add | B::Sub | B::Rem, ux, uy) if ux == uy => Some(ux),
                (B::Mul, Some(u), None) | (B::Mul, None, Some(u)) => Some(Some(u)),
                (B::Mul, None, None) => Some(None),
                _ => None,
            };
            if let Some(unit) = unit {
                let d = match op {
                    B::Add => x.add(&y),
                    B::Sub => x.sub(&y),
                    B::Mul => x.mul(&y),
                    _ => x.rem(&y),
                }
                .map_err(dec_fail)?;
                return Ok(Value::of_dec(d, unit.map(str::to_string)));
            }
        }
        let (ka, kb) = (self.kind_of(a)?, self.kind_of(b)?);
        let (x, y) = (self.to_f64(a)?, self.to_f64(b)?);
        match op {
            B::Add => self.real(x + y, ka.or(kb)),
            B::Sub => self.real(x - y, ka.or(kb)),
            B::Rem if y == 0.0 => Err(EvalKind::DivZero.into()),
            B::Rem => self.real(x % y, ka.or(kb)),
            B::Mul => self.real(x * y, self.product_kind(ka, kb, 1)?),
            B::Div if y == 0.0 => Err(EvalKind::DivZero.into()),
            B::Div => self.real(x / y, self.product_kind(ka, kb, -1)?),
            _ => Err(Fail::Internal),
        }
    }

    /// The order of two numeric values (REQ-004).
    pub(crate) fn cmp(&self, l: &Value, r: &Value) -> Result<Ordering, Fail> {
        let (a, b) = (num(l).ok_or(Fail::Internal)?, num(r).ok_or(Fail::Internal)?);
        self.cmp_num(a, b)
    }

    pub(crate) fn cmp_num(&self, a: Num<'_>, b: Num<'_>) -> Result<Ordering, Fail> {
        match (a, b) {
            (Num::Int(x), Num::Int(y)) => Ok(x.cmp(&y)),
            (Num::Real(x, _), Num::Real(y, _)) => x.partial_cmp(&y).ok_or(Fail::Internal),
            (Num::Real(x, _), Num::Int(i)) => cmp_real_plain(x, &Dec::from_i64(i)),
            (Num::Real(x, _), Num::Dec(d, None)) => cmp_real_plain(x, d),
            (Num::Real(x, _), Num::Dec(d, Some(u))) => cmp_real_dec(x, d, u, self.cat),
            (Num::Int(_) | Num::Dec(..), Num::Real(..)) => self.cmp_num(b, a).map(Ordering::reverse),
            (a, b) => {
                let ((x, ux), (y, uy)) = (exact(a).ok_or(Fail::Internal)?, exact(b).ok_or(Fail::Internal)?);
                match (ux, uy) {
                    (ux, uy) if ux == uy => Ok(x.cmp(&y)),
                    (Some(_), Some(_)) => {
                        let (p, q) = (self.to_f64(a)?, self.to_f64(b)?);
                        p.partial_cmp(&q).ok_or(Fail::Internal)
                    }
                    _ => Err(Fail::Internal),
                }
            }
        }
    }

    /// `==` (REQ-004 for numbers; by value otherwise).
    pub(crate) fn equal(&self, l: &Value, r: &Value) -> Result<bool, Fail> {
        let mut stack = vec![(l, r)];
        while let Some((a, b)) = stack.pop() {
            let same = match (a.v(), b.v()) {
                (V::Int(_) | V::Dec(..) | V::Real(..), _) => self.cmp(a, b)? == Ordering::Equal,
                (V::Bool(x), V::Bool(y)) => x == y,
                (V::Str(x) | V::Enum(x), V::Str(y) | V::Enum(y)) => x == y,
                (V::Element(x), V::Element(y)) => x == y,
                (V::List(xs), V::List(ys)) => {
                    if xs.len() != ys.len() {
                        false
                    } else {
                        stack.extend(xs.iter().zip(ys.iter()));
                        true
                    }
                }
                (V::Record(xs), V::Record(ys)) => {
                    if !xs.keys().eq(ys.keys()) {
                        false
                    } else {
                        stack.extend(xs.values().zip(ys.values()));
                        true
                    }
                }
                _ => return Err(Fail::Internal),
            };
            if !same {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

/// Whether `v` is a value of `ty` (explicit stack; records need every typed field and no
/// undeclared one; `formula` has no runtime value here).
pub(crate) fn conforms(kinds: &Kinds, ty: &Type, v: &Value) -> bool {
    let mut stack = vec![(ty, v)];
    while let Some((t, v)) = stack.pop() {
        let ok = match (t, v.v()) {
            (Type::Bool, V::Bool(_)) | (Type::Int, V::Int(_)) | (Type::Str, V::Str(_)) => true,
            (Type::Element(_), V::Element(_)) => true,
            (Type::Decimal, V::Dec(_, None) | V::Real(_, None)) => true,
            (Type::Quantity(k), V::Dec(_, Some(u))) => {
                kinds.unit(u).is_some_and(|uk| Kind::Named(uk.to_string()) == *k)
            }
            (Type::Quantity(k), V::Real(_, Some(vk))) => k == vk,
            (Type::Enum(set), V::Enum(m)) => set.contains(m),
            (Type::List(t), V::List(xs)) => {
                stack.extend(xs.iter().map(|x| (&**t, x)));
                true
            }
            (Type::Record(r), V::Record(m)) => {
                let declared = m.keys().all(|k| r.fields.contains_key(k));
                let mut ok = declared;
                for (name, ft) in &r.fields {
                    match (ft, m.get(name)) {
                        (Some(ft), Some(x)) => stack.push((ft, x)),
                        (Some(_), None) => ok = false,
                        (None, _) => {}
                    }
                }
                ok
            }
            _ => false,
        };
        if !ok {
            return false;
        }
    }
    true
}
