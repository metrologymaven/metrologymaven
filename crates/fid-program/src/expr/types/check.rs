//! The program walk: declarations, scopes, steps, and the manifest's parameter and result
//! schemas.

use super::infer::{infer, Kinds};
use super::{Diag, DiagCode, ExprDiag, Kind, RecordType, Site, Type, TypeReport, Typed};
use crate::expr::syntax::{parse_expr, ExprKind, Name, ParseError, Span};
use crate::family::ParamType;
use crate::fmt::{Arg, LoopMode, Opaque, Program, Step, StepKind, Uuid7};
use fid_canon::CValue;
use fid_registry::{CommandRef, EntityType, Manifest, QuantityCatalogue};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

/// Deepest `list<...>` nesting of a type source, and deepest schema nesting mapped to a type.
pub const MAX_TYPE_DEPTH: usize = 16;
/// Longest type source or name echoed in a diagnostic, in bytes (cut at a char boundary).
const MAX_ECHO: usize = 64;

/// Types every expression of `program` against the commands of `manifest` and the kinds and
/// units of `catalogue` (REQ-AUTO.PROG.EXPR.TYPES-001 to -007). See the module docs for the
/// rules and the diagnostic order. Never fails; a malformed part is a diagnostic.
pub fn check_types(program: &Program, manifest: &Manifest, catalogue: &dyn QuantityCatalogue) -> TypeReport {
    // The catalogue must be the one the manifest was built against, and its unit symbols
    // must name one kind each; otherwise nothing is typed.
    let digest = catalogue.digest();
    if manifest.quantity_catalogue_digest != digest {
        let names = vec![manifest.quantity_catalogue_digest.to_string(), digest.to_string()];
        let diagnostics =
            vec![Diag { code: DiagCode::CatalogueMismatch, step: None, slot: "catalogue".into(), span: None, names }];
        return TypeReport { types: BTreeMap::new(), diagnostics };
    }
    let kinds = match Kinds::new(catalogue) {
        Ok(k) => k,
        Err(sym) => {
            let names = vec![echo(&sym)];
            let diagnostics = vec![Diag {
                code: DiagCode::CatalogueInvalid,
                step: None,
                slot: "catalogue".into(),
                span: None,
                names,
            }];
            return TypeReport { types: BTreeMap::new(), diagnostics };
        }
    };
    let mut w = Walker { kinds, manifest, scopes: Vec::new(), subs: BTreeMap::new(), report: TypeReport::default() };
    w.globals(program);
    w.signatures(program);
    w.block(&program.main, &Ret::Main, None);
    let globals = w.scopes.clone();
    for sub in &program.subroutines {
        w.scopes = globals.clone();
        let mut frame = BTreeMap::new();
        if let Some(sig) = w.subs.get(&sub.name) {
            for (p, t) in &sig.params {
                frame.entry(p.clone()).or_insert_with(|| Var::new(t.clone(), false));
            }
        }
        let ret = match w.subs.get(&sub.name) {
            Some(sig) => Ret::Sub(sig.returns.clone()),
            None => Ret::Sub(None),
        };
        w.block(&sub.body, &ret, Some(frame));
    }
    w.report
}

/// What a `return` in this body must yield: nothing (`main`), or the subroutine's declared
/// type (`None` = no `returns`; `Some(None)` = a `returns` that did not type).
enum Ret {
    Main,
    Sub(Option<Option<Type>>),
}

struct Sig {
    params: Vec<(String, Option<Type>)>,
    returns: Option<Option<Type>>,
}

/// A manifest parameter as the checker sees it.
struct ParamInfo {
    ty: Option<Type>,
    value_of_record: bool,
}

/// A visible name: its type (`None` if already reported as bad) and whether `assign` may
/// change it (part-family parameters and counted-loop variables may not).
#[derive(Debug, Clone)]
struct Var {
    ty: Option<Type>,
    read_only: bool,
}

impl Var {
    fn new(ty: Option<Type>, read_only: bool) -> Var {
        Var { ty, read_only }
    }
}

type Frame = BTreeMap<String, Var>;

struct Walker<'a> {
    kinds: Kinds,
    manifest: &'a Manifest,
    scopes: Vec<Frame>,
    subs: BTreeMap<String, Sig>,
    report: TypeReport,
}

fn echo(s: &str) -> String {
    let mut end = s.len().min(MAX_ECHO);
    while !s.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    s.get(..end).unwrap_or_default().to_string()
}

impl Walker<'_> {
    fn decl_diag(&mut self, code: DiagCode, slot: String, names: Vec<String>) {
        self.report.diagnostics.push(Diag { code, step: None, slot, span: None, names });
    }

    fn step_diag(&mut self, code: DiagCode, step: Uuid7, slot: &str, span: Option<Span>, names: Vec<String>) {
        self.report.diagnostics.push(Diag { code, step: Some(step), slot: slot.to_string(), span, names });
    }

    fn lookup(&self, name: &str) -> Option<Var> {
        self.scopes.iter().rev().find_map(|f| f.get(name)).cloned()
    }

    /// Declares in the innermost block; `false` if that block already has the name.
    fn declare(&mut self, name: &str, ty: Option<Type>) -> bool {
        self.declare_var(name, Var::new(ty, false))
    }

    fn declare_var(&mut self, name: &str, var: Var) -> bool {
        match self.scopes.last_mut() {
            Some(f) if f.contains_key(name) => false,
            Some(f) => {
                f.insert(name.to_string(), var);
                true
            }
            None => {
                self.scopes.push(BTreeMap::from([(name.to_string(), var)]));
                true
            }
        }
    }

    // ---------------------------------------------------------------- declarations

    fn globals(&mut self, program: &Program) {
        self.scopes.push(BTreeMap::new());
        for p in &program.params {
            let Some(CValue::Str(name)) = p.get("name") else { continue };
            let ty = self.param_decl_type(p);
            if !self.declare_var(name, Var::new(ty, true)) {
                self.decl_diag(DiagCode::Redeclared, format!("params.{name}"), vec![echo(name)]);
            }
        }
        for v in &program.variables {
            let ty = self.type_source(&v.ty, &format!("variables.{}", v.name));
            if !self.declare(&v.name, ty) {
                self.decl_diag(DiagCode::Redeclared, format!("variables.{}", v.name), vec![echo(&v.name)]);
            }
        }
    }

    /// The type of a part-family parameter declaration. A declaration `AUTO.PROG.FAMILY`
    /// refuses is declared without a type, so its uses are not reported twice.
    fn param_decl_type(&self, p: &Opaque) -> Option<Type> {
        let Some(CValue::Str(t)) = p.get("type") else { return None };
        Some(match ParamType::parse(t)? {
            ParamType::Boolean => Type::Bool,
            ParamType::Integer => Type::Int,
            ParamType::Number => Type::Decimal,
            ParamType::String => Type::Str,
            ParamType::Quantity => match p.get("quantity_kind") {
                Some(CValue::Str(k)) if self.kinds.has(k) => Type::Quantity(Kind::Named(k.clone())),
                _ => return None,
            },
            ParamType::Ref(e) => Type::Element(e.as_str().to_string()),
            ParamType::DataHandle => Type::Element("data_handle".into()),
        })
    }

    fn signatures(&mut self, program: &Program) {
        for sub in &program.subroutines {
            let base = format!("subroutines.{}", sub.name);
            if self.subs.contains_key(&sub.name) {
                self.decl_diag(DiagCode::Redeclared, base, vec![echo(&sub.name)]);
                continue;
            }
            let mut params: Vec<(String, Option<Type>)> = Vec::new();
            for p in &sub.params {
                let slot = format!("{base}.params.{}", p.name);
                let ty = self.type_source(&p.ty, &slot);
                if params.iter().any(|(n, _)| *n == p.name) {
                    self.decl_diag(DiagCode::Redeclared, slot, vec![echo(&p.name)]);
                } else {
                    params.push((p.name.clone(), ty));
                }
            }
            let returns = sub.returns.as_ref().map(|r| self.type_source(r, &format!("{base}.returns")));
            self.subs.insert(sub.name.clone(), Sig { params, returns });
        }
    }

    /// Parses a type source; `type_unknown` at `slot` if it does not.
    fn type_source(&mut self, src: &str, slot: &str) -> Option<Type> {
        let t = parse_type(src, &self.kinds, self.manifest, 0);
        if t.is_none() {
            self.decl_diag(DiagCode::TypeUnknown, slot.to_string(), vec![echo(src)]);
        }
        t
    }

    // ---------------------------------------------------------------- steps

    /// Walks `steps` in a new block, seeded with `frame`.
    fn block(&mut self, steps: &[Step], ret: &Ret, frame: Option<Frame>) {
        self.scopes.push(frame.unwrap_or_default());
        for s in steps {
            self.step(s, ret);
        }
        self.scopes.pop();
    }

    /// Types one expression source at `step`/`slot`; records its type. Returns the type,
    /// the expression's span and, when the whole expression is a string literal, its text.
    /// A source that does not parse is `type_unknown` with the parser's code (span at the
    /// offending offset for a syntax error).
    fn expr(&mut self, step: Uuid7, slot: &str, src: &str) -> Value {
        let ast = match parse_expr(src) {
            Ok(a) => a,
            Err(e) => {
                let span = match e {
                    ParseError::Syntax { offset } => Some(Span::new(offset, offset)),
                    ParseError::TooLarge(_) => None,
                };
                self.step_diag(DiagCode::TypeUnknown, step, slot, span, vec![e.code().to_string()]);
                return Value { ty: None, span: None, literal: None };
            }
        };
        let mut diags: Vec<ExprDiag> = Vec::new();
        let scopes = &self.scopes;
        let env = |n: &str| scopes.iter().rev().find_map(|f| f.get(n)).map(|v| v.ty.clone());
        let ty = infer(&ast, &env, &self.kinds, &mut diags);
        for d in diags {
            self.step_diag(d.code, step, slot, Some(d.span), d.names);
        }
        if let Some(t) = &ty {
            let site = Site { step, slot: slot.to_string() };
            self.report.types.insert(site, Typed { ty: t.clone(), span: ast.span, not_of_record: true });
        }
        let literal = match &ast.kind {
            ExprKind::Str(s) => Some(s.clone()),
            _ => None,
        };
        Value { ty, span: Some(ast.span), literal }
    }

    /// `type_mismatch` unless `v` may be stored where `target` is declared (REQ-007: equal
    /// types, `int` into `decimal`, an `enum` into a superset, a member string literal into
    /// an `enum`). Silent when `v` has no type (already reported).
    fn assign_check(&mut self, step: Uuid7, slot: &str, target: &Type, v: &Value) {
        let Some(t) = &v.ty else { return };
        let member = matches!((target, t, &v.literal), (Type::Enum(set), Type::Str, Some(l)) if set.contains(l));
        if !member && !assignable(target, t) {
            let found = match (target, &v.literal) {
                (Type::Enum(_), Some(l)) => format!("{l:?}"),
                _ => t.to_string(),
            };
            self.step_diag(DiagCode::TypeMismatch, step, slot, v.span, vec![target.to_string(), found]);
        }
    }

    fn want(&mut self, step: Uuid7, slot: &str, src: &str, want: Type) {
        let v = self.expr(step, slot, src);
        if let Some(t) = v.ty {
            if t != want {
                self.step_diag(DiagCode::TypeMismatch, step, slot, v.span, vec![want.to_string(), t.to_string()]);
            }
        }
    }

    fn step(&mut self, s: &Step, ret: &Ret) {
        let id = s.step_id;
        match &s.kind {
            StepKind::Invoke { command, major, args, bind } => self.invoke(id, command, *major, args, bind.as_deref()),
            StepKind::Assign { var, expr } => {
                let v = self.expr(id, "expr", expr);
                match self.lookup(var) {
                    None => self.step_diag(DiagCode::Undeclared, id, "var", None, vec![echo(var)]),
                    Some(Var { read_only: true, .. }) => {
                        self.step_diag(DiagCode::ReadOnly, id, "var", None, vec![echo(var)]);
                    }
                    Some(Var { ty: Some(vt), .. }) => self.assign_check(id, "expr", &vt, &v),
                    Some(Var { ty: None, .. }) => {}
                }
            }
            StepKind::If { branches, else_body } => {
                for (i, b) in branches.iter().enumerate() {
                    self.want(id, &format!("branches.{i}.cond"), &b.cond, Type::Bool);
                    self.block(&b.body, ret, None);
                }
                self.block(else_body, ret, None);
            }
            StepKind::Loop { mode, body, .. } => match mode {
                LoopMode::Counted { var, from, to, step } => {
                    for (slot, src) in [("from", from), ("to", to), ("step", step)] {
                        self.want(id, slot, src, Type::Int);
                    }
                    self.block(body, ret, Some(BTreeMap::from([(var.clone(), Var::new(Some(Type::Int), true))])));
                }
                LoopMode::While { cond } => {
                    self.want(id, "cond", cond, Type::Bool);
                    self.block(body, ret, None);
                }
            },
            StepKind::Call { subroutine, args, bind } => self.call(id, subroutine, args, bind.as_deref()),
            StepKind::Return { expr } => self.ret(id, expr.as_deref(), ret),
            StepKind::Jump { cond: Some(c), .. } => self.want(id, "cond", c, Type::Bool),
            StepKind::Jump { cond: None, .. }
            | StepKind::Label { .. }
            | StepKind::Operator { .. }
            | StepKind::Comment { .. } => {}
        }
    }

    fn invoke(&mut self, id: Uuid7, command: &str, major: u32, args: &BTreeMap<String, Arg>, bind: Option<&str>) {
        let key = CommandRef { id: command.to_string(), major };
        let manifest = self.manifest;
        let def = manifest.commands.get(&key);
        let label = format!("{command}@{major}");
        if def.is_none() {
            self.step_diag(DiagCode::TypeUnknown, id, "command", None, vec![echo(&label)]);
        }
        let params = def.map(|d| self.params(&d.params_schema)).unwrap_or_default();
        for (name, arg) in args {
            let Arg::Expr(src) = arg else { continue };
            let slot = format!("args.{name}");
            let v = self.expr(id, &slot, src);
            match params.get(name) {
                Some(p) if p.value_of_record => {
                    self.step_diag(DiagCode::ExpressionValueOfRecord, id, &slot, v.span, vec![echo(name)]);
                }
                Some(ParamInfo { ty: Some(pt), .. }) => self.assign_check(id, &slot, pt, &v),
                _ => {}
            }
        }
        if let Some(b) = bind {
            let result = def.and_then(|d| schema_type(&d.result_schema, &label, &self.kinds, 0));
            if def.is_some() && result.is_none() {
                self.step_diag(DiagCode::TypeUnknown, id, "bind", None, vec![echo(&label)]);
            }
            if !self.declare(b, result) {
                self.step_diag(DiagCode::Redeclared, id, "bind", None, vec![echo(b)]);
            }
        }
    }

    fn params(&self, schema: &CValue) -> BTreeMap<String, ParamInfo> {
        let mut out = BTreeMap::new();
        let Some(CValue::Obj(props)) = obj(schema).and_then(|m| m.get("properties")) else { return out };
        for (name, s) in props {
            let ty = schema_type(s, name, &self.kinds, 0);
            out.insert(name.clone(), ParamInfo { ty, value_of_record: value_of_record(s, schema, 0) });
        }
        out
    }

    fn call(&mut self, id: Uuid7, subroutine: &str, args: &BTreeMap<String, Arg>, bind: Option<&str>) {
        let sig = self.subs.get(subroutine).map(|s| (s.params.clone(), s.returns.clone()));
        if sig.is_none() {
            self.step_diag(DiagCode::Undeclared, id, "subroutine", None, vec![echo(subroutine)]);
        }
        for (name, arg) in args {
            let Arg::Expr(src) = arg else { continue };
            let slot = format!("args.{name}");
            let v = self.expr(id, &slot, src);
            let Some((params, _)) = &sig else { continue };
            match params.iter().find(|(p, _)| p == name) {
                None => self.step_diag(DiagCode::Undeclared, id, &slot, None, vec![echo(name)]),
                Some((_, Some(pt))) => self.assign_check(id, &slot, pt, &v),
                Some((_, None)) => {}
            }
        }
        if let Some(b) = bind {
            let ty = match &sig {
                Some((_, Some(r))) => r.clone(),
                Some((_, None)) => {
                    self.step_diag(DiagCode::TypeUnknown, id, "bind", None, vec![echo(subroutine)]);
                    None
                }
                None => None,
            };
            if !self.declare(b, ty) {
                self.step_diag(DiagCode::Redeclared, id, "bind", None, vec![echo(b)]);
            }
        }
    }

    fn ret(&mut self, id: Uuid7, expr: Option<&str>, ret: &Ret) {
        let got = expr.map(|src| self.expr(id, "expr", src));
        match (ret, got) {
            (Ret::Main | Ret::Sub(None), Some(Value { ty: Some(t), span, .. })) => {
                self.step_diag(DiagCode::TypeMismatch, id, "expr", span, vec!["none".into(), t.to_string()]);
            }
            (Ret::Sub(Some(Some(rt))), None) => {
                self.step_diag(DiagCode::TypeMismatch, id, "expr", None, vec![rt.to_string(), "none".into()]);
            }
            (Ret::Sub(Some(Some(rt))), Some(v)) => {
                let rt = rt.clone();
                self.assign_check(id, "expr", &rt, &v);
            }
            _ => {}
        }
    }
}

/// A typed top-level expression: its type (`None` if already reported), span, and its text
/// when it is a single string literal.
struct Value {
    ty: Option<Type>,
    span: Option<Span>,
    literal: Option<String>,
}

/// Whether a parameter schema holds a value of record anywhere inside it: a schema with
/// `"value_of_record": true` (CORE.REG.DEF.SCHEMA REQ-011), found through `items`, `prefixItems`,
/// `allOf`, `anyOf`, `oneOf`, `properties` and local `$ref`s (`#/$defs/<name>` of the
/// command's `params_schema`). Fails closed: a `$ref` that does not resolve, or nesting
/// deeper than [`MAX_TYPE_DEPTH`], counts as a value of record.
pub fn value_of_record(s: &CValue, root: &CValue, depth: usize) -> bool {
    if depth > MAX_TYPE_DEPTH {
        return true;
    }
    let Some(m) = obj(s) else { return false };
    if m.get("value_of_record") == Some(&CValue::Bool(true)) {
        return true;
    }
    let next = depth.saturating_add(1);
    if let Some(r) = m.get("$ref") {
        let target = match r {
            CValue::Str(r) => r
                .strip_prefix("#/$defs/")
                .and_then(|name| obj(root)?.get("$defs").and_then(obj).and_then(|d| d.get(name))),
            _ => None,
        };
        match target {
            Some(t) if value_of_record(t, root, next) => return true,
            Some(_) => {}
            None => return true,
        }
    }
    if m.get("items").is_some_and(|i| value_of_record(i, root, next)) {
        return true;
    }
    for key in ["prefixItems", "allOf", "anyOf", "oneOf"] {
        if let Some(CValue::Arr(items)) = m.get(key) {
            if items.iter().any(|i| value_of_record(i, root, next)) {
                return true;
            }
        }
    }
    if let Some(CValue::Obj(props)) = m.get("properties") {
        if props.values().any(|p| value_of_record(p, root, next)) {
            return true;
        }
    }
    false
}

/// Whether `value` may be stored where `target` is declared.
pub fn assignable(target: &Type, value: &Type) -> bool {
    match (target, value) {
        (Type::Decimal, Type::Int) => true,
        (Type::Enum(t), Type::Enum(v)) => v.is_subset(t),
        (t, v) => t == v,
    }
}

fn obj(v: &CValue) -> Option<&BTreeMap<String, CValue>> {
    match v {
        CValue::Obj(m) => Some(m),
        _ => None,
    }
}

fn is_entity(e: &str) -> bool {
    EntityType::parse(e).is_some() || e == "data_handle"
}

/// The static type of a manifest JSON Schema, or `None` when it has none: `x-quantity` (a
/// catalogue kind) is a quantity, `x-ref` an element, `boolean`/`integer`/`number`/`string`
/// the scalar types, a `string` (or untyped) schema whose `enum` holds only strings an
/// `enum`, `array` with `items` a list, `object` a record of its `properties`. `$ref`, type
/// unions and anything else have no static type.
pub(crate) fn schema_type(s: &CValue, name: &str, kinds: &Kinds, depth: usize) -> Option<Type> {
    if depth > MAX_TYPE_DEPTH {
        return None;
    }
    let m = obj(s)?;
    if let Some(q) = m.get("x-quantity") {
        return match q {
            CValue::Str(k) if kinds.has(k) => Some(Type::Quantity(Kind::Named(k.clone()))),
            _ => None,
        };
    }
    if let Some(r) = m.get("x-ref") {
        return match r {
            CValue::Str(e) if is_entity(e) => Some(Type::Element(e.clone())),
            _ => None,
        };
    }
    let enum_of = || -> Option<Type> {
        let CValue::Arr(items) = m.get("enum")? else { return None };
        let set: BTreeSet<String> = items
            .iter()
            .map(|i| match i {
                CValue::Str(s) => Some(s.clone()),
                _ => None,
            })
            .collect::<Option<_>>()?;
        (!set.is_empty()).then_some(Type::Enum(set))
    };
    match m.get("type") {
        Some(CValue::Str(t)) => match t.as_str() {
            "boolean" => Some(Type::Bool),
            "integer" => Some(Type::Int),
            "number" => Some(Type::Decimal),
            "string" if m.contains_key("enum") => enum_of(),
            "string" => Some(Type::Str),
            "array" => {
                let item = schema_type(m.get("items")?, name, kinds, depth.saturating_add(1))?;
                Some(Type::List(Box::new(item)))
            }
            "object" => {
                let mut fields = BTreeMap::new();
                if let Some(CValue::Obj(props)) = m.get("properties") {
                    for (k, v) in props {
                        let child = format!("{name}.{k}");
                        fields.insert(k.clone(), schema_type(v, &child, kinds, depth.saturating_add(1)));
                    }
                }
                Some(Type::Record(Arc::new(RecordType { name: name.to_string(), fields })))
            }
            _ => None,
        },
        None => enum_of(),
        Some(_) => None,
    }
}

/// Parses a type source: `bool`, `int`, `decimal`, `string`, `quantity<kind>`,
/// `formula<kind>`, `enum<a|b|...>` (identifiers, no repeat), `element<entity>`,
/// `list<T>` (at most [`MAX_TYPE_DEPTH`] deep) or `record<command@major>`. No spaces.
pub(crate) fn parse_type(src: &str, kinds: &Kinds, manifest: &Manifest, depth: usize) -> Option<Type> {
    if depth > MAX_TYPE_DEPTH {
        return None;
    }
    match src {
        "bool" => return Some(Type::Bool),
        "int" => return Some(Type::Int),
        "decimal" => return Some(Type::Decimal),
        "string" => return Some(Type::Str),
        _ => {}
    }
    let (head, rest) = src.split_once('<')?;
    let inner = rest.strip_suffix('>')?;
    match head {
        "quantity" if kinds.has(inner) => Some(Type::Quantity(Kind::Named(inner.to_string()))),
        "formula" if kinds.has(inner) => Some(Type::Formula(Kind::Named(inner.to_string()))),
        "element" if is_entity(inner) => Some(Type::Element(inner.to_string())),
        "list" => parse_type(inner, kinds, manifest, depth.saturating_add(1)).map(|t| Type::List(Box::new(t))),
        "enum" => {
            let mut set = BTreeSet::new();
            for v in inner.split('|') {
                Name::new(v)?;
                if !set.insert(v.to_string()) {
                    return None;
                }
            }
            Some(Type::Enum(set))
        }
        "record" => {
            let (id, major) = inner.rsplit_once('@')?;
            if major.is_empty() || !major.bytes().all(|b| b.is_ascii_digit()) || major.len() > 9 {
                return None;
            }
            let major: u32 = major.parse().ok()?;
            let def = manifest.commands.get(&CommandRef { id: id.to_string(), major })?;
            match schema_type(&def.result_schema, inner, kinds, 0)? {
                t @ Type::Record(_) => Some(t),
                _ => None,
            }
        }
        _ => None,
    }
}
