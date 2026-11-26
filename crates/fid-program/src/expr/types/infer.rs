//! Bottom-up typing of one expression tree. The walk uses an explicit stack, so the deepest
//! tree the parser builds (a flat chain of about 512 binary operators) is safe.

use super::{kind_dim, DiagCode, Dim, ExprDiag, Kind, Type};
use crate::expr::syntax::{BinaryOp, Expr, ExprKind, Span, UnaryOp};
use fid_registry::QuantityCatalogue;
use std::collections::{BTreeMap, BTreeSet};

/// The function library, sorted, with its arity (`None` = two or more for `min`/`max`, one
/// or more for `format`). Signatures (types only; `AUTO.PROG.EXPR.EVAL` evaluates them):
///
/// - `abs(x)`: `int`, `decimal` or quantity, same type back;
/// - `min(a, b, ...)`, `max(a, b, ...)`: the `+` rule over all arguments;
/// - `round(x)`, `floor(x)`, `ceil(x)`: `int` or `decimal` to `int`;
/// - `sqrt(x)`: a number to `decimal`; a quantity to the kind of the halved dimension
///   (`type_mismatch` if an exponent is odd);
/// - `sin`, `cos`, `tan`: a plain number (radians) or an angle, to `decimal`;
/// - `atan2(y, x)`: the `+` rule on `y` and `x`, to `quantity<angle>`;
/// - `len(x)`: a `list` or `string` to `int`;
/// - `contains(list<T>, T)` or `contains(string, string)`: `bool`;
/// - `format(string, any...)`: `string`;
/// - `to_unit(quantity<k>, "<unit symbol of k>")`: `quantity<k>`; the symbol must be a
///   string literal (`unknown_unit` if the catalogue lacks it, `unit_mismatch` if it is a
///   unit of another kind).
pub const LIBRARY: [(&str, Option<usize>); 15] = [
    ("abs", Some(1)),
    ("atan2", Some(2)),
    ("ceil", Some(1)),
    ("contains", Some(2)),
    ("cos", Some(1)),
    ("floor", Some(1)),
    ("format", None),
    ("len", Some(1)),
    ("max", None),
    ("min", None),
    ("round", Some(1)),
    ("sin", Some(1)),
    ("sqrt", Some(1)),
    ("tan", Some(1)),
    ("to_unit", Some(2)),
];

/// The kinds and unit symbols of a catalogue, with their pinned dimensions.
pub(crate) struct Kinds {
    dims: BTreeMap<String, Option<Dim>>,
    units: BTreeMap<String, String>,
    by_dim: BTreeMap<Dim, Vec<String>>,
}

impl Kinds {
    /// Fails with the first unit symbol (byte order) that two kinds share: such a symbol
    /// has no single kind, so the catalogue is refused rather than resolved last-wins.
    pub(crate) fn new(cat: &dyn QuantityCatalogue) -> Result<Kinds, String> {
        let mut k = Kinds { dims: BTreeMap::new(), units: BTreeMap::new(), by_dim: BTreeMap::new() };
        let mut dup = BTreeSet::new();
        for name in cat.kind_names() {
            let d = kind_dim(name);
            k.dims.insert(name.to_string(), d);
            if let Some(d) = d {
                k.by_dim.entry(d).or_default().push(name.to_string());
            }
            if let Some(info) = cat.kind(name) {
                for u in &info.units {
                    if let Some(prev) = k.units.insert(u.symbol.clone(), name.to_string()) {
                        if prev != name {
                            dup.insert(u.symbol.clone());
                        }
                    }
                }
            }
        }
        match dup.into_iter().next() {
            Some(sym) => Err(sym),
            None => Ok(k),
        }
    }

    /// Whether the catalogue has this kind.
    pub(crate) fn has(&self, kind: &str) -> bool {
        self.dims.contains_key(kind)
    }

    /// The kind of a unit symbol (exact, case-sensitive).
    pub(crate) fn unit(&self, sym: &str) -> Option<&str> {
        self.units.get(sym).map(String::as_str)
    }

    pub(crate) fn dim_of(&self, k: &Kind) -> Option<Dim> {
        match k {
            Kind::Named(n) => self.dims.get(n).copied().flatten(),
            Kind::Derived(d) => Some(*d),
        }
    }

    /// `decimal` for the empty dimension, the one catalogue kind with this dimension, or a
    /// derived kind.
    pub(crate) fn type_of_dim(&self, d: Dim) -> Type {
        if d.is_none() {
            return Type::Decimal;
        }
        match self.by_dim.get(&d).map(Vec::as_slice) {
            Some([one]) => Type::Quantity(Kind::Named(one.clone())),
            _ => Type::Quantity(Kind::Derived(d)),
        }
    }
}

/// Types one expression against a plain variable map. Returns the type (`None` if a
/// diagnostic made it underivable) and the diagnostics, ordered by span.
pub fn type_of(
    expr: &Expr,
    vars: &BTreeMap<String, Type>,
    cat: &dyn QuantityCatalogue,
) -> (Option<Type>, Vec<ExprDiag>) {
    let kinds = match Kinds::new(cat) {
        Ok(k) => k,
        Err(sym) => {
            return (None, vec![ExprDiag { code: DiagCode::CatalogueInvalid, span: expr.span, names: vec![sym] }]);
        }
    };
    let mut out = Vec::new();
    let t = infer(expr, &|n: &str| vars.get(n).map(|t| Some(t.clone())), &kinds, &mut out);
    (t, out)
}

/// Name lookup: `None` if the name is not visible, `Some(None)` if it is but its type is
/// already known to be bad (no further diagnostic).
pub(crate) type Env<'a> = &'a dyn Fn(&str) -> Option<Option<Type>>;

enum Work<'e> {
    Visit(&'e Expr),
    Done(&'e Expr),
}

fn children(e: &Expr) -> Vec<&Expr> {
    match &e.kind {
        ExprKind::Field(b, _) => vec![&**b],
        ExprKind::Index(b, i) => vec![&**b, &**i],
        ExprKind::Unary(_, x) => vec![&**x],
        ExprKind::Binary(_, l, r) => vec![&**l, &**r],
        ExprKind::Cond(c, a, b) => vec![&**c, &**a, &**b],
        ExprKind::Call(_, args) => args.iter().collect(),
        _ => Vec::new(),
    }
}

/// Types `expr`, appending its diagnostics to `out` sorted by span (stable).
pub(crate) fn infer(expr: &Expr, env: Env<'_>, kinds: &Kinds, out: &mut Vec<ExprDiag>) -> Option<Type> {
    let mut t = Typer { kinds, env, diags: Vec::new() };
    let mut work = vec![Work::Visit(expr)];
    let mut vals: Vec<Option<Type>> = Vec::new();
    while let Some(w) = work.pop() {
        match w {
            Work::Visit(e) => {
                work.push(Work::Done(e));
                work.extend(children(e).into_iter().rev().map(Work::Visit));
            }
            Work::Done(e) => {
                let n = children(e).len();
                let args = vals.split_off(vals.len().saturating_sub(n));
                let ty = t.node(e, args);
                vals.push(ty);
            }
        }
    }
    let mut diags = t.diags;
    diags.sort_by_key(|d| (d.span.start, d.span.end));
    out.extend(diags);
    vals.pop().flatten()
}

/// The string-literal text of the two operands, when they are literals.
type Lits<'e> = (Option<&'e str>, Option<&'e str>);

fn literal(e: &Expr) -> Option<&str> {
    match &e.kind {
        ExprKind::Str(s) => Some(s),
        _ => None,
    }
}

struct Typer<'a> {
    kinds: &'a Kinds,
    env: Env<'a>,
    diags: Vec<ExprDiag>,
}

const NUMBER: &str = "number";

fn name_of(t: &Type) -> String {
    match t {
        Type::Quantity(k) => k.to_string(),
        t if t.is_number() => NUMBER.into(),
        t => t.to_string(),
    }
}

impl Typer<'_> {
    fn diag(&mut self, code: DiagCode, span: Span, names: Vec<String>) -> Option<Type> {
        self.diags.push(ExprDiag { code, span, names });
        None
    }

    fn mismatch(&mut self, span: Span, expected: &str, found: &Type) -> Option<Type> {
        self.diag(DiagCode::TypeMismatch, span, vec![expected.to_string(), found.to_string()])
    }

    fn node(&mut self, e: &Expr, args: Vec<Option<Type>>) -> Option<Type> {
        let sp = e.span;
        match &e.kind {
            ExprKind::Int(_) => Some(Type::Int),
            ExprKind::Dec(_) => Some(Type::Decimal),
            ExprKind::Str(_) => Some(Type::Str),
            ExprKind::Bool(_) => Some(Type::Bool),
            ExprKind::Qty(_, u) => match self.kinds.unit(u.as_str()) {
                Some(k) => Some(Type::Quantity(Kind::Named(k.to_string()))),
                None => self.diag(DiagCode::UnknownUnit, sp, vec![u.as_str().to_string()]),
            },
            ExprKind::Ident(n) => match (self.env)(n.as_str()) {
                Some(t) => t,
                None => self.diag(DiagCode::Undeclared, sp, vec![n.as_str().to_string()]),
            },
            ExprKind::Call(name, exprs) => self.call(sp, name.as_str(), exprs, args),
            _ => {
                // Every other node needs all its operands typed; a bad operand was reported.
                let args: Vec<Type> = args.into_iter().collect::<Option<_>>()?;
                self.compound(e, &args)
            }
        }
    }

    fn compound(&mut self, e: &Expr, a: &[Type]) -> Option<Type> {
        let sp = e.span;
        match (&e.kind, a) {
            (ExprKind::Field(_, name), [base]) => match base {
                Type::Record(r) => match r.fields.get(name.as_str()) {
                    Some(Some(t)) => Some(t.clone()),
                    _ => self.diag(DiagCode::TypeUnknown, sp, vec![base.to_string(), name.as_str().to_string()]),
                },
                other => self.diag(DiagCode::TypeUnknown, sp, vec![other.to_string(), name.as_str().to_string()]),
            },
            (ExprKind::Index(..), [base, idx]) => match base {
                Type::List(t) if *idx == Type::Int => Some((**t).clone()),
                Type::List(_) => self.mismatch(sp, "int", idx),
                other => self.mismatch(sp, "list", other),
            },
            (ExprKind::Unary(UnaryOp::Neg, _), [x]) => match x {
                Type::Int | Type::Decimal | Type::Quantity(_) => Some(x.clone()),
                other => self.mismatch(sp, NUMBER, other),
            },
            (ExprKind::Unary(UnaryOp::Not, _), [x]) => self.boolean(sp, x),
            (ExprKind::Binary(op, le, re), [l, r]) => {
                let lits = (literal(le), literal(re));
                self.binary(sp, *op, l, r, lits)
            }
            (ExprKind::Cond(..), [c, x, y]) => {
                self.boolean(sp, c)?;
                self.unify(sp, x, y)
            }
            // Arity is fixed by the parser; any other shape cannot occur.
            _ => self.diag(DiagCode::TypeUnknown, sp, Vec::new()),
        }
    }

    fn boolean(&mut self, sp: Span, t: &Type) -> Option<Type> {
        match t {
            Type::Bool => Some(Type::Bool),
            other => self.mismatch(sp, "bool", other),
        }
    }

    fn binary(&mut self, sp: Span, op: BinaryOp, l: &Type, r: &Type, lits: Lits<'_>) -> Option<Type> {
        use BinaryOp as B;
        match op {
            B::And | B::Or => {
                self.boolean(sp, l)?;
                self.boolean(sp, r)
            }
            B::Mul => self.product(sp, l, r, 1),
            B::Div => self.product(sp, l, r, -1),
            B::Add | B::Sub | B::Rem => self.additive(sp, l, r),
            B::Lt | B::Le | B::Gt | B::Ge => self.compare(sp, l, r).map(|_| Type::Bool),
            B::Eq | B::Ne => self.equality(sp, l, r, lits).map(|_| Type::Bool),
        }
    }

    /// `+ - %`, `min`, `max`: equal kinds, no derived kind, no plain number with a quantity.
    fn additive(&mut self, sp: Span, l: &Type, r: &Type) -> Option<Type> {
        match (l, r) {
            (Type::Quantity(Kind::Derived(_)), _) => self.mismatch(sp, "quantity of a catalogue kind", l),
            (_, Type::Quantity(Kind::Derived(_))) => self.mismatch(sp, "quantity of a catalogue kind", r),
            _ => self.compare(sp, l, r),
        }
    }

    /// Comparisons: like [`Typer::additive`] but a derived kind meets the same derived kind.
    /// Returns the common type.
    fn compare(&mut self, sp: Span, l: &Type, r: &Type) -> Option<Type> {
        match (l, r) {
            (Type::Int, Type::Int) => Some(Type::Int),
            (a, b) if a.is_number() && b.is_number() => Some(Type::Decimal),
            (Type::Quantity(a), Type::Quantity(b)) if a == b => Some(l.clone()),
            (Type::Quantity(_), b) | (b, Type::Quantity(_)) if b.is_number() || matches!(b, Type::Quantity(_)) => {
                self.diag(DiagCode::UnitMismatch, sp, vec![name_of(l), name_of(r)])
            }
            (a, b) => {
                let bad = if a.is_number() || matches!(a, Type::Quantity(_)) { b } else { a };
                self.mismatch(sp, NUMBER, bad)
            }
        }
    }

    /// `==`, `!=`. An `enum` meets an `enum` sharing at least one member, or a string
    /// literal that is one of its members; never a computed `string`.
    fn equality(&mut self, sp: Span, l: &Type, r: &Type, lits: Lits<'_>) -> Option<Type> {
        let numeric = |t: &Type| t.is_number() || matches!(t, Type::Quantity(_));
        match (l, r) {
            (a, b) if numeric(a) || numeric(b) => self.compare(sp, l, r),
            (Type::Enum(a), Type::Enum(b)) if a.intersection(b).next().is_some() => Some(Type::Bool),
            (Type::Enum(_), Type::Enum(_)) => self.mismatch(sp, &l.to_string(), r),
            (Type::Enum(set), Type::Str) | (Type::Str, Type::Enum(set)) => {
                let (e, lit) = if matches!(l, Type::Enum(_)) { (l, lits.1) } else { (r, lits.0) };
                match lit {
                    Some(v) if set.contains(v) => Some(Type::Bool),
                    Some(v) => self.diag(DiagCode::TypeMismatch, sp, vec![e.to_string(), format!("{v:?}")]),
                    None => self.mismatch(sp, &e.to_string(), &Type::Str),
                }
            }
            (a, b) if a == b && !matches!(a, Type::Formula(_)) => Some(Type::Bool),
            (a, b) => self.mismatch(sp, &a.to_string(), b),
        }
    }

    /// `if c then x else y`: the common type of both branches.
    fn unify(&mut self, sp: Span, x: &Type, y: &Type) -> Option<Type> {
        let numeric = |t: &Type| t.is_number() || matches!(t, Type::Quantity(_));
        match (x, y) {
            (a, b) if numeric(a) || numeric(b) => self.compare(sp, x, y),
            (Type::Enum(a), Type::Enum(b)) => Some(Type::Enum(a.union(b).cloned().collect())),
            (a, b) if a == b => Some(a.clone()),
            (a, b) => self.mismatch(sp, &a.to_string(), b),
        }
    }

    /// `*` (`sign` 1) and `/` (`sign` -1).
    fn product(&mut self, sp: Span, l: &Type, r: &Type, sign: i8) -> Option<Type> {
        match (l, r) {
            // `/` of two `int`s is `decimal` (maintainer decision on PR #418); integer
            // division is written `floor(a / b)`.
            (Type::Int, Type::Int) if sign > 0 => Some(Type::Int),
            (a, b) if a.is_number() && b.is_number() => Some(Type::Decimal),
            (Type::Quantity(_), b) if b.is_number() => Some(l.clone()),
            (a, Type::Quantity(_)) if a.is_number() && sign > 0 => Some(r.clone()),
            (a, Type::Quantity(k)) if a.is_number() => self.scaled(sp, Dim::NONE, k, sign),
            (Type::Quantity(a), Type::Quantity(b)) => match self.kinds.dim_of(a) {
                Some(da) => self.scaled(sp, da, b, sign),
                None => self.diag(DiagCode::TypeUnknown, sp, vec![a.to_string()]),
            },
            (a, b) => {
                let bad = if a.is_number() || matches!(a, Type::Quantity(_)) { b } else { a };
                self.mismatch(sp, NUMBER, bad)
            }
        }
    }

    fn scaled(&mut self, sp: Span, da: Dim, b: &Kind, sign: i8) -> Option<Type> {
        let Some(db) = self.kinds.dim_of(b) else {
            return self.diag(DiagCode::TypeUnknown, sp, vec![b.to_string()]);
        };
        match da.combine(db, sign) {
            Some(d) => Some(self.kinds.type_of_dim(d)),
            None => self.diag(DiagCode::TypeUnknown, sp, vec!["dimension_overflow".into()]),
        }
    }

    fn call(&mut self, sp: Span, name: &str, exprs: &[Expr], args: Vec<Option<Type>>) -> Option<Type> {
        let Some(arity) = LIBRARY.iter().find(|(n, _)| *n == name).map(|e| e.1) else {
            return self.diag(DiagCode::TypeUnknown, sp, vec![name.to_string()]);
        };
        let n = args.len();
        let ok = match (name, arity) {
            (_, Some(k)) => n == k,
            ("format", None) => n >= 1,
            _ => n >= 2,
        };
        if !ok {
            let want = match arity {
                Some(k) => format!("{name}/{k}"),
                None if name == "format" => format!("{name}/1+"),
                None => format!("{name}/2+"),
            };
            return self.diag(DiagCode::TypeMismatch, sp, vec![want, format!("{name}/{n}")]);
        }
        let a: Vec<Type> = args.into_iter().collect::<Option<_>>()?;
        match (name, a.as_slice()) {
            ("abs", [x]) => match x {
                Type::Int | Type::Decimal | Type::Quantity(_) => Some(x.clone()),
                other => self.mismatch(sp, NUMBER, other),
            },
            ("min" | "max", [first, rest @ ..]) => {
                let mut acc = first.clone();
                for t in rest {
                    acc = self.additive(sp, &acc, t)?;
                }
                Some(acc)
            }
            ("round" | "floor" | "ceil", [x]) if x.is_number() => Some(Type::Int),
            ("round" | "floor" | "ceil", [x]) => self.mismatch(sp, NUMBER, x),
            ("sqrt", [x]) if x.is_number() => Some(Type::Decimal),
            ("sqrt", [q @ Type::Quantity(k)]) => match self.kinds.dim_of(k) {
                None => self.diag(DiagCode::TypeUnknown, sp, vec![k.to_string()]),
                Some(d) => match d.half() {
                    Some(h) => Some(self.kinds.type_of_dim(h)),
                    None => self.mismatch(sp, "quantity with even exponents", q),
                },
            },
            ("sqrt", [x]) => self.mismatch(sp, NUMBER, x),
            ("sin" | "cos" | "tan", [x]) if x.is_number() => Some(Type::Decimal),
            ("sin" | "cos" | "tan", [Type::Quantity(k)]) if self.kinds.dim_of(k) == Some(Dim::ANGLE) => {
                Some(Type::Decimal)
            }
            ("sin" | "cos" | "tan", [Type::Quantity(k)]) => {
                self.diag(DiagCode::UnitMismatch, sp, vec!["angle".into(), k.to_string()])
            }
            ("sin" | "cos" | "tan", [x]) => self.mismatch(sp, "angle", x),
            ("atan2", [y, x]) => {
                self.compare(sp, y, x)?;
                Some(self.kinds.type_of_dim(Dim::ANGLE))
            }
            ("len", [Type::List(_) | Type::Str]) => Some(Type::Int),
            ("len", [x]) => self.mismatch(sp, "list", x),
            ("contains", [Type::Str, Type::Str]) => Some(Type::Bool),
            ("contains", [Type::List(t), v]) => {
                let lit = exprs.get(1).and_then(literal);
                self.equality(sp, t, v, (None, lit)).map(|_| Type::Bool)
            }
            ("contains", [x, _]) => self.mismatch(sp, "list", x),
            ("format", [Type::Str, ..]) => Some(Type::Str),
            ("format", [x, ..]) => self.mismatch(sp, "string", x),
            ("to_unit", [Type::Quantity(k), Type::Str]) => {
                let Some(ExprKind::Str(sym)) = exprs.get(1).map(|e| &e.kind) else {
                    return self.mismatch(sp, "string literal", &Type::Str);
                };
                match self.kinds.unit(sym) {
                    None => self.diag(DiagCode::UnknownUnit, sp, vec![sym.clone()]),
                    Some(uk) if Kind::Named(uk.to_string()) == *k => Some(Type::Quantity(k.clone())),
                    Some(uk) => self.diag(DiagCode::UnitMismatch, sp, vec![k.to_string(), uk.to_string()]),
                }
            }
            ("to_unit", [Type::Quantity(_), x]) => self.mismatch(sp, "string", x),
            ("to_unit", [x, _]) => self.mismatch(sp, "quantity", x),
            _ => self.diag(DiagCode::TypeUnknown, sp, vec![name.to_string()]),
        }
    }
}
