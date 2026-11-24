//! The reader: classify, index the link entities, build the items, link presentations.

use crate::names::{self, Class};
use crate::{
    PmiBound, PmiClass, PmiError, PmiGraph, PmiItem, PmiKind, PmiOptions, PmiValue, ValueUnit, IDENT_COST, INDEX_COST,
    ITEM_COST, LINK_COST, PARAM_COST, VALUE_COST,
};
use fid_io_step::{Instance, P21File, Param, StepHeaderInfo};
use std::collections::{BTreeMap, BTreeSet};

/// Reads the PMI graph of `file` with the default bounds.
pub fn read_pmi(file: &P21File, header: &StepHeaderInfo) -> Result<PmiGraph, PmiError> {
    read_pmi_with(file, header, &PmiOptions::default())
}

/// Reads the PMI graph of `file` within `opts`.
pub fn read_pmi_with(file: &P21File, header: &StepHeaderInfo, opts: &PmiOptions) -> Result<PmiGraph, PmiError> {
    let mut meter = Meter { alloc: 0, work: 0, opts: *opts };
    let idx = Index::build(file, opts, &mut meter)?;
    let mut ctx = Ctx { file, header, idx: &idx, meter: &mut meter };
    let mut items = BTreeMap::new();
    for (&n, &class) in &idx.classes {
        ctx.meter.step(1)?;
        let Some(inst) = file.data.get(&n) else { continue };
        let mut item = ctx.new_item(inst, class)?;
        match class {
            Class::Supported(kind) => {
                ctx.scan(inst, &mut item, true)?;
                if kind == PmiKind::DimensionRepresentation {
                    ctx.representation_values(inst, &mut item)?;
                }
                if kind.has_geometry() {
                    ctx.closure(n, &mut item)?;
                }
            }
            Class::Unsupported(..) => ctx.scan(inst, &mut item, false)?,
            Class::Presentation | Class::NotPmi => {}
        }
        items.insert(n, item);
    }
    ctx.associations(&mut items)?;
    for item in items.values_mut() {
        for list in [&mut item.links, &mut item.link_unresolved, &mut item.refs, &mut item.presented_by] {
            list.sort_unstable();
            list.dedup();
        }
        if item.kind == PmiKind::Presentation && item.class != PmiClass::Unsupported {
            item.class = if item.refs.is_empty() { PmiClass::GraphicOnly } else { PmiClass::Semantic };
        }
    }
    Ok(PmiGraph { items })
}

/// Allocation and work accounting.
struct Meter {
    alloc: u64,
    work: u64,
    opts: PmiOptions,
}

impl Meter {
    fn charge(&mut self, bytes: u64) -> Result<(), PmiError> {
        self.alloc = self.alloc.saturating_add(bytes);
        if self.alloc > self.opts.max_alloc_bytes {
            return Err(PmiError::LimitExceeded { bound: PmiBound::Alloc, value: self.opts.max_alloc_bytes });
        }
        Ok(())
    }

    fn release(&mut self, bytes: u64) {
        self.alloc = self.alloc.saturating_sub(bytes);
    }

    fn step(&mut self, n: u64) -> Result<(), PmiError> {
        self.work = self.work.saturating_add(n);
        if self.work > self.opts.max_work {
            return Err(PmiError::LimitExceeded { bound: PmiBound::Work, value: self.opts.max_work });
        }
        Ok(())
    }
}

fn malformed(entity: u64, reason: &'static str) -> PmiError {
    PmiError::Malformed { entity, reason }
}

/// Attribute `i` of the part named `root` (complex form), or of the only part.
fn attr<'a>(inst: &'a Instance, root: &str, i: usize) -> Option<&'a Param> {
    let params = match inst.parts.iter().find(|(n, _)| n.as_str() == root) {
        Some((_, p)) => p,
        None => match inst.parts.as_slice() {
            [(_, p)] => p,
            _ => return None,
        },
    };
    params.get(i)
}

fn req_ref(inst: &Instance, root: &str, i: usize, reason: &'static str) -> Result<u64, PmiError> {
    match attr(inst, root, i) {
        Some(Param::Ref(n)) => Ok(*n),
        _ => Err(malformed(inst.number, reason)),
    }
}

fn has(inst: &Instance, name: &str) -> bool {
    inst.names().any(|n| n.as_str() == name)
}

fn has_any(inst: &Instance, names: &[&str]) -> bool {
    inst.names().any(|n| names.contains(&n.as_str()))
}

fn is_measure(inst: &Instance) -> bool {
    inst.names().any(|n| n.as_str().ends_with("MEASURE_WITH_UNIT"))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum UnitKind {
    Length,
    Angle,
    Other,
}

fn unit_kind(inst: &Instance) -> UnitKind {
    if has(inst, "LENGTH_UNIT") {
        return UnitKind::Length;
    }
    if has(inst, "PLANE_ANGLE_UNIT") {
        return UnitKind::Angle;
    }
    for (name, params) in &inst.parts {
        if name.as_str() != "SI_UNIT" {
            continue;
        }
        if let Some(Param::Enum(e)) = params.last() {
            match e.as_str() {
                "METRE" => return UnitKind::Length,
                "RADIAN" => return UnitKind::Angle,
                _ => {}
            }
        }
    }
    UnitKind::Other
}

/// The first list parameter of the `GLOBAL_UNIT_ASSIGNED_CONTEXT` part.
fn assigned_units(inst: &Instance) -> Option<&[Param]> {
    let (_, params) = inst.parts.iter().find(|(n, _)| n.as_str() == "GLOBAL_UNIT_ASSIGNED_CONTEXT")?;
    params.iter().find_map(|p| match p {
        Param::List(xs) => Some(xs.as_slice()),
        _ => None,
    })
}

/// The classification of every instance and the link-entity indexes.
struct Index {
    /// PMI items only.
    classes: BTreeMap<u64, Class>,
    /// Shape aspect (usage definition) to identified items, for the geometry usages.
    usages: BTreeMap<u64, Vec<u64>>,
    /// Associations: (definition, identified items).
    assocs: Vec<(u64, Vec<u64>)>,
    /// Shape-aspect relationships, relating to related and back.
    fwd: BTreeMap<u64, Vec<u64>>,
    rev: BTreeMap<u64, Vec<u64>>,
    /// Unit entity to the lowest context assigning it, and every (context, unit) pair.
    unit_ctx: BTreeMap<u64, u64>,
    pairs: BTreeSet<(u64, u64)>,
}

impl Index {
    fn build(file: &P21File, opts: &PmiOptions, meter: &mut Meter) -> Result<Index, PmiError> {
        let mut idx = Index {
            classes: BTreeMap::new(),
            usages: BTreeMap::new(),
            assocs: Vec::new(),
            fwd: BTreeMap::new(),
            rev: BTreeMap::new(),
            unit_ctx: BTreeMap::new(),
            pairs: BTreeSet::new(),
        };
        for (&n, inst) in &file.data {
            meter.step(1)?;
            let names: Vec<_> = inst.names().collect();
            let class = names::classify(&names);
            if class != Class::NotPmi {
                if idx.classes.len() >= opts.max_items {
                    return Err(PmiError::LimitExceeded {
                        bound: PmiBound::Items,
                        value: u64::try_from(opts.max_items).unwrap_or(u64::MAX),
                    });
                }
                meter.charge(INDEX_COST)?;
                idx.classes.insert(n, class);
            }
        }
        for (&n, inst) in &file.data {
            meter.step(1)?;
            if idx.classes.contains_key(&n) {
                continue;
            }
            if has_any(inst, names::USAGE_NAMES) {
                let def = req_ref(inst, names::USAGE_ROOT, 2, "usage_definition")?;
                let items = match attr(inst, names::USAGE_ROOT, 4) {
                    Some(Param::Ref(x)) => vec![*x],
                    Some(Param::List(xs)) => {
                        let mut out = Vec::new();
                        for x in xs {
                            meter.step(1)?;
                            match x {
                                Param::Ref(x) => out.push(*x),
                                _ => return Err(malformed(n, "usage_item")),
                            }
                        }
                        out
                    }
                    _ => return Err(malformed(n, "usage_item")),
                };
                meter.charge(INDEX_COST.saturating_add(LINK_COST.saturating_mul(len64(items.len()))))?;
                if has_any(inst, names::ASSOCIATION_NAMES) {
                    idx.assocs.push((def, items));
                } else {
                    idx.usages.entry(def).or_default().extend(items);
                }
            } else if has(inst, "SHAPE_ASPECT_RELATIONSHIP") {
                let a = req_ref(inst, "SHAPE_ASPECT_RELATIONSHIP", 2, "relationship_relating")?;
                let b = req_ref(inst, "SHAPE_ASPECT_RELATIONSHIP", 3, "relationship_related")?;
                meter.charge(INDEX_COST.saturating_mul(2))?;
                idx.fwd.entry(a).or_default().push(b);
                idx.rev.entry(b).or_default().push(a);
            }
            if let Some(units) = assigned_units(inst) {
                for u in units {
                    meter.step(1)?;
                    if let Param::Ref(u) = u {
                        meter.charge(INDEX_COST.saturating_mul(2))?;
                        idx.pairs.insert((n, *u));
                        let e = idx.unit_ctx.entry(*u).or_insert(n);
                        if n < *e {
                            *e = n;
                        }
                    }
                }
            }
        }
        Ok(idx)
    }

    fn is_presentation(&self, n: u64) -> bool {
        matches!(self.classes.get(&n), Some(Class::Presentation))
    }
}

fn len64(n: usize) -> u64 {
    u64::try_from(n).unwrap_or(u64::MAX)
}

struct Ctx<'a> {
    file: &'a P21File,
    header: &'a StepHeaderInfo,
    idx: &'a Index,
    meter: &'a mut Meter,
}

impl Ctx<'_> {
    fn defined(&self, n: u64) -> bool {
        self.file.data.contains_key(&n)
    }

    fn push(&mut self, list: &mut Vec<u64>, n: u64) -> Result<(), PmiError> {
        self.meter.charge(LINK_COST)?;
        list.push(n);
        Ok(())
    }

    fn new_item(&mut self, inst: &Instance, class: Class) -> Result<PmiItem, PmiError> {
        let (class, kind, unsupported) = match class {
            Class::Supported(k) => (PmiClass::Semantic, k, None),
            Class::Unsupported(k, r) => (PmiClass::Unsupported, k, Some(r)),
            Class::Presentation | Class::NotPmi => (PmiClass::GraphicOnly, PmiKind::Presentation, None),
        };
        self.meter.charge(ITEM_COST)?;
        let types: Vec<_> = inst.names().cloned().collect();
        self.meter.charge(IDENT_COST.saturating_mul(len64(types.len())))?;
        // Charge the raw copy before making it: every parameter node and its text.
        let mut stack: Vec<&Param> = Vec::new();
        for (_, params) in &inst.parts {
            self.meter.charge(IDENT_COST)?;
            stack.extend(params.iter());
            while let Some(p) = stack.pop() {
                self.meter.step(1)?;
                let text = match p {
                    Param::Str(t) => len64(t.len()),
                    Param::Bin(b) => len64(b.hex_digits().len()),
                    Param::Real(_, Some(l)) => len64(l.as_str().len()),
                    Param::Typed(_, inner) => {
                        stack.push(inner);
                        0
                    }
                    Param::List(xs) => {
                        stack.extend(xs.iter());
                        0
                    }
                    _ => 0,
                };
                self.meter.charge(PARAM_COST.saturating_add(text))?;
            }
        }
        Ok(PmiItem {
            entity: inst.number,
            types,
            class,
            kind,
            unsupported,
            links: Vec::new(),
            link_unresolved: Vec::new(),
            refs: Vec::new(),
            presented_by: Vec::new(),
            values: Vec::new(),
            raw: inst.parts.clone(),
        })
    }

    /// Every reference in the instance's parameters, in depth-first pre-order: undefined
    /// numbers to `link_unresolved`, PMI items to `refs`, measures to `values`.
    fn scan(&mut self, inst: &Instance, item: &mut PmiItem, with_values: bool) -> Result<(), PmiError> {
        let file = self.file;
        let mut stack: Vec<&Param> = Vec::new();
        for (_, params) in inst.parts.iter().rev() {
            stack.extend(params.iter().rev());
        }
        let mut seen = BTreeSet::new();
        let mut charged = 0u64;
        while let Some(p) = stack.pop() {
            self.meter.step(1)?;
            match p {
                Param::List(xs) => stack.extend(xs.iter().rev()),
                Param::Typed(_, inner) => stack.push(inner),
                Param::Ref(r) => {
                    let r = *r;
                    if !self.defined(r) {
                        self.push(&mut item.link_unresolved, r)?;
                    } else if self.idx.classes.contains_key(&r) {
                        self.push(&mut item.refs, r)?;
                    } else if with_values && file.data.get(&r).is_some_and(is_measure) && seen.insert(r) {
                        self.meter.charge(INDEX_COST)?;
                        charged = charged.saturating_add(INDEX_COST);
                        self.value(item, r, None)?;
                    }
                }
                _ => {}
            }
        }
        self.meter.release(charged);
        Ok(())
    }

    /// Adds the value of measure `m` unless the item already has it. `ctx` is the
    /// representation context for a dimension's representation items; otherwise the
    /// lowest context assigning the measure's unit is used.
    fn value(&mut self, item: &mut PmiItem, m: u64, ctx: Option<Option<u64>>) -> Result<(), PmiError> {
        if item.values.iter().any(|v| v.measure == m) {
            return Ok(());
        }
        let file = self.file;
        let Some(mi) = file.data.get(&m) else { return Ok(()) };
        let vp = attr(mi, "MEASURE_WITH_UNIT", 0).ok_or_else(|| malformed(m, "measure_value"))?;
        let u = req_ref(mi, "MEASURE_WITH_UNIT", 1, "measure_unit")?;
        let (measure_type, inner) = match vp {
            Param::Typed(t, inner) => (Some(t.clone()), &**inner),
            other => (None, other),
        };
        let lexeme: Option<Box<str>> = match inner {
            Param::Real(_, Some(l)) => Some(Box::from(l.as_str())),
            Param::Int(i) => Some(i.to_string().into_boxed_str()),
            _ => None,
        };
        let unit_inst = file.data.get(&u);
        if unit_inst.is_none() {
            self.push(&mut item.link_unresolved, u)?;
        }
        let unit_ctx = match ctx {
            Some(c) => c,
            None => self.idx.unit_ctx.get(&u).copied(),
        };
        let mut unit = ValueUnit::Unresolved;
        if let (Some(ui), Some(c)) = (unit_inst, unit_ctx) {
            if self.idx.pairs.contains(&(c, u)) {
                if let Some(uc) = self.header.contexts.get(&c) {
                    unit = match unit_kind(ui) {
                        UnitKind::Length => ValueUnit::Length(uc.length_unit),
                        UnitKind::Angle => uc.angle.map_or(ValueUnit::Unresolved, ValueUnit::Angle),
                        UnitKind::Other => ValueUnit::Unresolved,
                    };
                }
            }
        }
        let text = lexeme.as_ref().map_or(0, |l| len64(l.len()));
        self.meter.charge(VALUE_COST.saturating_add(text))?;
        item.values.push(PmiValue { measure: m, measure_type, lexeme, unit_entity: Some(u), unit_ctx, unit });
        Ok(())
    }

    /// The values of a dimensional characteristic representation's representation items.
    fn representation_values(&mut self, inst: &Instance, item: &mut PmiItem) -> Result<(), PmiError> {
        let rep = req_ref(inst, "DIMENSIONAL_CHARACTERISTIC_REPRESENTATION", 1, "dimension_representation")?;
        let file = self.file;
        let Some(ri) = file.data.get(&rep) else { return Ok(()) };
        let ctx = match attr(ri, "REPRESENTATION", 2) {
            Some(Param::Ref(c)) => Some(*c),
            _ => None,
        };
        let Some(Param::List(xs)) = attr(ri, "REPRESENTATION", 1) else {
            return Err(malformed(rep, "representation_items"));
        };
        for x in xs {
            self.meter.step(1)?;
            let Param::Ref(x) = x else { return Err(malformed(rep, "representation_items")) };
            let x = *x;
            match file.data.get(&x) {
                None => self.push(&mut item.link_unresolved, x)?,
                Some(xi) if is_measure(xi) => self.value(item, x, Some(ctx))?,
                Some(_) => {}
            }
        }
        Ok(())
    }

    /// The faces and edges the item's shape aspects resolve to (REQ-IO.CAD.PMI.READ-004).
    fn closure(&mut self, start: u64, item: &mut PmiItem) -> Result<(), PmiError> {
        let mut seen = BTreeSet::new();
        let mut todo = vec![start];
        let mut charged = LINK_COST;
        self.meter.charge(LINK_COST)?;
        let result = self.closure_loop(&mut seen, &mut todo, &mut charged, item);
        self.meter.release(charged);
        result
    }

    fn closure_loop(
        &mut self,
        seen: &mut BTreeSet<u64>,
        todo: &mut Vec<u64>,
        charged: &mut u64,
        item: &mut PmiItem,
    ) -> Result<(), PmiError> {
        let file = self.file;
        let idx = self.idx;
        while let Some(a) = todo.pop() {
            self.meter.step(1)?;
            if !seen.insert(a) {
                continue;
            }
            self.meter.charge(INDEX_COST)?;
            *charged = charged.saturating_add(INDEX_COST);
            let Some(ai) = file.data.get(&a) else {
                self.push(&mut item.link_unresolved, a)?;
                continue;
            };
            if let Some(xs) = idx.usages.get(&a) {
                for &x in xs {
                    self.meter.step(1)?;
                    if self.defined(x) {
                        self.push(&mut item.links, x)?;
                    } else {
                        self.push(&mut item.link_unresolved, x)?;
                    }
                }
            }
            let Some(Class::Supported(kind)) = idx.classes.get(&a) else { continue };
            let mut next: Vec<u64> = Vec::new();
            match kind {
                PmiKind::Tolerance => next.push(req_ref(ai, "GEOMETRIC_TOLERANCE", 3, "tolerance_target")?),
                PmiKind::Dimension if has(ai, "DIMENSIONAL_SIZE") => {
                    next.push(req_ref(ai, "DIMENSIONAL_SIZE", 0, "dimension_aspect")?)
                }
                PmiKind::Dimension => {
                    next.push(req_ref(ai, "SHAPE_ASPECT_RELATIONSHIP", 2, "dimension_aspect")?);
                    next.push(req_ref(ai, "SHAPE_ASPECT_RELATIONSHIP", 3, "dimension_aspect")?);
                }
                _ => {}
            }
            if has_any(ai, names::COMPOSITE_ASPECTS) {
                if let Some(members) = idx.fwd.get(&a) {
                    next.extend(members.iter().copied());
                }
            }
            if *kind == PmiKind::Datum {
                if let Some(features) = idx.rev.get(&a) {
                    next.extend(features.iter().copied());
                }
            }
            let cost = LINK_COST.saturating_mul(len64(next.len()));
            self.meter.step(len64(next.len()))?;
            self.meter.charge(cost)?;
            *charged = charged.saturating_add(cost);
            todo.extend(next);
        }
        Ok(())
    }

    /// Links presentations to the PMI items their associations name.
    fn associations(&mut self, items: &mut BTreeMap<u64, PmiItem>) -> Result<(), PmiError> {
        let idx = self.idx;
        let file = self.file;
        for (def, targets) in &idx.assocs {
            let def = *def;
            let def_semantic = matches!(idx.classes.get(&def), Some(Class::Supported(_) | Class::Unsupported(..)));
            for &p in targets {
                self.meter.step(1)?;
                if !self.defined(p) {
                    if def_semantic {
                        self.add(items, def, Field::Unresolved, p)?;
                    }
                    continue;
                }
                if !idx.is_presentation(p) {
                    continue;
                }
                if !self.defined(def) {
                    self.add(items, p, Field::Unresolved, def)?;
                    continue;
                }
                if !def_semantic {
                    continue;
                }
                self.link(items, p, def)?;
                let Some(pi) = file.data.get(&p) else { continue };
                if !pi.names().any(|n| names::is_callout_name(n.as_str())) {
                    continue;
                }
                let Some(Param::List(members)) = attr(pi, "DRAUGHTING_CALLOUT", 1) else {
                    return Err(malformed(p, "callout_contents"));
                };
                for m in members {
                    self.meter.step(1)?;
                    let Param::Ref(m) = m else { return Err(malformed(p, "callout_contents")) };
                    let m = *m;
                    if !self.defined(m) {
                        self.add(items, p, Field::Unresolved, m)?;
                    } else if idx.is_presentation(m) {
                        self.link(items, m, def)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn link(&mut self, items: &mut BTreeMap<u64, PmiItem>, p: u64, def: u64) -> Result<(), PmiError> {
        self.add(items, p, Field::Refs, def)?;
        self.add(items, def, Field::PresentedBy, p)
    }

    fn add(&mut self, items: &mut BTreeMap<u64, PmiItem>, at: u64, field: Field, n: u64) -> Result<(), PmiError> {
        let Some(item) = items.get_mut(&at) else { return Ok(()) };
        let list = match field {
            Field::Refs => &mut item.refs,
            Field::PresentedBy => &mut item.presented_by,
            Field::Unresolved => &mut item.link_unresolved,
        };
        self.push(list, n)
    }
}

#[derive(Clone, Copy)]
enum Field {
    Refs,
    PresentedBy,
    Unresolved,
}
