//! Container entities and hierarchy rules. Spec node `CORE.PRJ.CONT.HIER`
//! (CONSTITUTION §5: Part → Revision → Plan → Run, stable IDs).
//!
//! - [`Part`], [`Revision`], [`Plan`] and [`Run`] are the only container kinds
//!   ([`ContainerKind`], REQ-001). A Revision has one Part, a Plan one Revision, and a Run one
//!   Plan and that Plan's Revision. Labels are untrusted text of 1 to [`MAX_LABEL_CHARS`]
//!   characters and need not be unique (REQ-006).
//! - [`create`] is the whole logic of the four create commands. It reads the state through a
//!   [`ContainerView`] (via the [`Hierarchy`] helper) and returns the new [`Container`]; it
//!   writes nothing. The bus handlers, registry definitions and the mapping to bus errors
//!   live in the engine crate `fid-project-cmd`, so this crate stays in the model layer and
//!   does not depend on the bus.
//! - Check order, first failure wins: parameter shape (`schema`), label (`label`),
//!   references (`bad_reference`: missing or of the wrong kind; for a Run the Plan before the
//!   Revision), the Plan-Revision rule (`hierarchy`, REQ-002), then the ID. Only a command
//!   that passes every check draws an ID, so a refused command records no observation.
//! - IDs (REQ-004): an imported `id` is kept unchanged through [`IdService::import_uuid`]
//!   (`id_collision` if the project has it); otherwise a UUIDv7 is drawn through
//!   [`IdService::new_v7`] from the [`ObservationSource`], which records its time and random
//!   bits for replay. A drawn ID the project already has also fails closed with
//!   `id_collision` (no retry).
//!
//! Wire form of an entity, also the body of its `ContainerCreated` event (RFC 8785):
//! `{"created_by": {"id", "kind"}, "id", "kind", "label", "parent"}` plus `"revision"` for a
//! Run. `kind` is the entity kind (`project.part`, ...); `label` is a string, or `null` for a
//! Plan or Run created without one; `parent` is `null` for a Part, else the Part of a
//! Revision, the Revision of a Plan or the Plan of a Run. IDs are canonical lowercase
//! hyphenated UUID text, which is also the entity ID the state view keys them by.

use crate::id::{IdError, IdIndex, IdService, ObservationSource, Uuid};
use fid_canon::CValue;
use std::collections::BTreeMap;
use std::fmt;

/// Longest label, in Unicode scalar values (REQ-006).
pub const MAX_LABEL_CHARS: usize = 256;
/// The type of the one event every create command returns.
pub const CONTAINER_CREATED: &str = "ContainerCreated";
/// Pattern of a canonical ID in the published schemas.
pub const ID_PATTERN: &str = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$";
/// Longest principal ID in `created_by`, in bytes (the bus principal limit).
pub const MAX_PRINCIPAL_ID_BYTES: usize = 256;
/// The principal kinds (CONSTITUTION §3, D-012), as the journal spells them.
pub const PRINCIPAL_KINDS: [&str; 6] = ["human", "agent", "script", "service", "device", "system"];

// ------------------------------------------------------------------ errors

/// Why a container command or a container body was refused. Codes are stable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContainerError {
    /// The parameters do not have the command's shape (`schema`).
    Schema(String),
    /// A label is empty or longer than [`MAX_LABEL_CHARS`] (`label`).
    Label,
    /// A named parent does not exist or is of the wrong kind (`bad_reference`).
    BadReference(String),
    /// A Run names a Plan of another Revision (`hierarchy`).
    Hierarchy(String),
    /// The ID is already in the project (`id_collision`).
    IdCollision(Uuid),
    /// The ID service failed for another reason (clock, CSPRNG, replay log).
    Id(IdError),
    /// A container entity in the project state, or a `created_by`, is malformed (`bad_state`).
    BadState(String),
}

impl ContainerError {
    /// Stable machine-readable code.
    pub fn code(&self) -> &'static str {
        match self {
            ContainerError::Schema(_) => "schema",
            ContainerError::Label => "label",
            ContainerError::BadReference(_) => "bad_reference",
            ContainerError::Hierarchy(_) => "hierarchy",
            ContainerError::IdCollision(_) => "id_collision",
            ContainerError::Id(e) => e.code(),
            ContainerError::BadState(_) => "bad_state",
        }
    }
}

impl fmt::Display for ContainerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ContainerError::Schema(m)
            | ContainerError::BadReference(m)
            | ContainerError::BadState(m)
            | ContainerError::Hierarchy(m) => write!(f, "{}: {m}", self.code()),
            ContainerError::Label => write!(f, "label: must be 1..={MAX_LABEL_CHARS} characters"),
            ContainerError::IdCollision(id) => write!(f, "id_collision: {id}"),
            ContainerError::Id(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ContainerError {}

impl From<IdError> for ContainerError {
    fn from(e: IdError) -> Self {
        match e {
            IdError::IdCollision(id) => ContainerError::IdCollision(id),
            IdError::BadId => ContainerError::Schema("id must not be the nil or max UUID".to_string()),
            other => ContainerError::Id(other),
        }
    }
}

fn schema(m: &str) -> ContainerError {
    ContainerError::Schema(m.to_string())
}

fn bad_state(m: &str) -> ContainerError {
    ContainerError::BadState(m.to_string())
}

// ------------------------------------------------------------------ kinds, labels, principals

/// The four container kinds (REQ-001).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ContainerKind {
    Part,
    Revision,
    Plan,
    Run,
}

impl ContainerKind {
    /// Every kind, top of the hierarchy first.
    pub const ALL: [ContainerKind; 4] =
        [ContainerKind::Part, ContainerKind::Revision, ContainerKind::Plan, ContainerKind::Run];

    /// The entity kind in the state view and the wire form.
    pub fn as_str(self) -> &'static str {
        match self {
            ContainerKind::Part => "project.part",
            ContainerKind::Revision => "project.revision",
            ContainerKind::Plan => "project.plan",
            ContainerKind::Run => "project.run",
        }
    }

    /// The kind with this entity kind string.
    pub fn parse(s: &str) -> Option<ContainerKind> {
        ContainerKind::ALL.into_iter().find(|k| k.as_str() == s)
    }

    /// Short name: `part`, `revision`, `plan`, `run` (parameter and result field names).
    pub fn short(self) -> &'static str {
        match self {
            ContainerKind::Part => "part",
            ContainerKind::Revision => "revision",
            ContainerKind::Plan => "plan",
            ContainerKind::Run => "run",
        }
    }

    /// The create command's name, without major: `project.create_part`, ...
    pub fn command(self) -> &'static str {
        match self {
            ContainerKind::Part => "project.create_part",
            ContainerKind::Revision => "project.create_revision",
            ContainerKind::Plan => "project.create_plan",
            ContainerKind::Run => "project.create_run",
        }
    }

    /// Whether the create command requires a label (Part and Revision do; a Plan or Run may
    /// be created without one).
    pub fn label_required(self) -> bool {
        matches!(self, ContainerKind::Part | ContainerKind::Revision)
    }
}

impl fmt::Display for ContainerKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A container label: untrusted text (CONSTITUTION C6) of 1 to [`MAX_LABEL_CHARS`]
/// characters. Stored and shown as given, never interpreted, not required to be unique.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Label(String);

impl Label {
    /// Checks the length; `label` if empty or too long.
    pub fn new(text: &str) -> Result<Label, ContainerError> {
        if text.is_empty() || text.chars().nth(MAX_LABEL_CHARS).is_some() {
            return Err(ContainerError::Label);
        }
        Ok(Label(text.to_string()))
    }

    /// The untrusted text, for display or storage only.
    pub fn as_untrusted_str(&self) -> &str {
        &self.0
    }
}

/// Who created a container: the principal's kind and ID, as the bus derived them
/// (`fid-project-cmd` builds it from the call's principal). Kind is one of
/// [`PRINCIPAL_KINDS`]; the ID is 1 to [`MAX_PRINCIPAL_ID_BYTES`] bytes with no control
/// character.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CreatedBy {
    kind: &'static str,
    id: String,
}

impl CreatedBy {
    /// Checks and wraps a principal kind and ID; `bad_state` if either is invalid.
    pub fn new(kind: &str, id: &str) -> Result<CreatedBy, ContainerError> {
        let kind =
            PRINCIPAL_KINDS.into_iter().find(|k| *k == kind).ok_or_else(|| bad_state("created_by kind unknown"))?;
        if id.is_empty() || id.len() > MAX_PRINCIPAL_ID_BYTES || id.chars().any(char::is_control) {
            return Err(bad_state("created_by id invalid"));
        }
        Ok(CreatedBy { kind, id: id.to_string() })
    }

    pub fn kind(&self) -> &str {
        self.kind
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    /// `{"id": <id>, "kind": <kind>}`.
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("id".to_string(), CValue::Str(self.id.clone()));
        o.insert("kind".to_string(), CValue::Str(self.kind.to_string()));
        CValue::Obj(o)
    }

    fn from_value(v: Option<&CValue>) -> Result<CreatedBy, ContainerError> {
        let Some(CValue::Obj(o)) = v else {
            return Err(bad_state("created_by must be an object"));
        };
        match (o.len(), o.get("id"), o.get("kind")) {
            (2, Some(CValue::Str(id)), Some(CValue::Str(kind))) => CreatedBy::new(kind, id),
            _ => Err(bad_state("created_by must be {id, kind}")),
        }
    }
}

// ------------------------------------------------------------------ entities

/// A Part: the top of the hierarchy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Part {
    pub id: Uuid,
    pub label: Label,
    pub created_by: CreatedBy,
}

/// A Revision of one Part.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Revision {
    pub id: Uuid,
    pub part: Uuid,
    pub label: Label,
    pub created_by: CreatedBy,
}

/// A Plan of one Revision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub id: Uuid,
    pub revision: Uuid,
    pub label: Option<Label>,
    pub created_by: CreatedBy,
}

/// A Run of one Plan and that Plan's Revision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    pub id: Uuid,
    pub plan: Uuid,
    pub revision: Uuid,
    pub label: Option<Label>,
    pub created_by: CreatedBy,
}

/// One container entity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Container {
    Part(Part),
    Revision(Revision),
    Plan(Plan),
    Run(Run),
}

fn uuid_text(id: &Uuid) -> CValue {
    CValue::Str(id.to_string())
}

impl Container {
    pub fn kind(&self) -> ContainerKind {
        match self {
            Container::Part(_) => ContainerKind::Part,
            Container::Revision(_) => ContainerKind::Revision,
            Container::Plan(_) => ContainerKind::Plan,
            Container::Run(_) => ContainerKind::Run,
        }
    }

    pub fn id(&self) -> Uuid {
        match self {
            Container::Part(p) => p.id,
            Container::Revision(r) => r.id,
            Container::Plan(p) => p.id,
            Container::Run(r) => r.id,
        }
    }

    /// The parent: none for a Part, else its Part, Revision or Plan.
    pub fn parent(&self) -> Option<Uuid> {
        match self {
            Container::Part(_) => None,
            Container::Revision(r) => Some(r.part),
            Container::Plan(p) => Some(p.revision),
            Container::Run(r) => Some(r.plan),
        }
    }

    pub fn label(&self) -> Option<&Label> {
        match self {
            Container::Part(p) => Some(&p.label),
            Container::Revision(r) => Some(&r.label),
            Container::Plan(p) => p.label.as_ref(),
            Container::Run(r) => r.label.as_ref(),
        }
    }

    pub fn created_by(&self) -> &CreatedBy {
        match self {
            Container::Part(p) => &p.created_by,
            Container::Revision(r) => &r.created_by,
            Container::Plan(p) => &p.created_by,
            Container::Run(r) => &r.created_by,
        }
    }

    /// The wire form (see the module docs).
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("created_by".to_string(), self.created_by().to_value());
        o.insert("id".to_string(), uuid_text(&self.id()));
        o.insert("kind".to_string(), CValue::Str(self.kind().as_str().to_string()));
        o.insert(
            "label".to_string(),
            self.label().map_or(CValue::Null, |l| CValue::Str(l.as_untrusted_str().to_string())),
        );
        o.insert("parent".to_string(), self.parent().map_or(CValue::Null, |p| uuid_text(&p)));
        if let Container::Run(r) = self {
            o.insert("revision".to_string(), uuid_text(&r.revision));
        }
        CValue::Obj(o)
    }

    /// The `ContainerCreated` event envelope `{"body": <wire form>, "type": "ContainerCreated"}`.
    pub fn created_event_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("body".to_string(), self.to_value());
        o.insert("type".to_string(), CValue::Str(CONTAINER_CREATED.to_string()));
        CValue::Obj(o)
    }

    /// Reads the wire form. Exactly the keys of its kind, canonical IDs, a valid label and
    /// principal, a null parent for a Part and a UUID parent otherwise; anything else is
    /// `bad_state`.
    pub fn from_value(v: &CValue) -> Result<Container, ContainerError> {
        let CValue::Obj(o) = v else {
            return Err(bad_state("container body must be an object"));
        };
        let kind = match o.get("kind") {
            Some(CValue::Str(s)) => ContainerKind::parse(s).ok_or_else(|| bad_state("unknown container kind"))?,
            _ => return Err(bad_state("container body lacks a kind")),
        };
        let expected = if kind == ContainerKind::Run { 6 } else { 5 };
        if o.len() != expected {
            return Err(bad_state("container body has missing or extra fields"));
        }
        let id = state_uuid(o.get("id"))?;
        let created_by = CreatedBy::from_value(o.get("created_by"))?;
        let label = match o.get("label") {
            Some(CValue::Null) if !kind.label_required() => None,
            Some(CValue::Str(s)) => Some(Label::new(s).map_err(|_| bad_state("container label out of range"))?),
            _ => return Err(bad_state("container label must be a string")),
        };
        let parent = match (kind, o.get("parent")) {
            (ContainerKind::Part, Some(CValue::Null)) => None,
            (ContainerKind::Part, _) => return Err(bad_state("a part has no parent")),
            (_, other) => Some(state_uuid(other)?),
        };
        let need = |l: Option<Label>| l.ok_or_else(|| bad_state("container label must be a string"));
        let parent_of = |p: Option<Uuid>| p.ok_or_else(|| bad_state("container parent missing"));
        Ok(match kind {
            ContainerKind::Part => Container::Part(Part { id, label: need(label)?, created_by }),
            ContainerKind::Revision => {
                Container::Revision(Revision { id, part: parent_of(parent)?, label: need(label)?, created_by })
            }
            ContainerKind::Plan => Container::Plan(Plan { id, revision: parent_of(parent)?, label, created_by }),
            ContainerKind::Run => Container::Run(Run {
                id,
                plan: parent_of(parent)?,
                revision: state_uuid(o.get("revision"))?,
                label,
                created_by,
            }),
        })
    }
}

/// A canonical lowercase hyphenated UUID, or `None` if `s` is anything else.
fn canonical_uuid(s: &str) -> Option<Uuid> {
    let id = Uuid::parse(s).ok()?;
    (id.to_string() == s).then_some(id)
}

fn state_uuid(v: Option<&CValue>) -> Result<Uuid, ContainerError> {
    match v {
        Some(CValue::Str(s)) => canonical_uuid(s).ok_or_else(|| bad_state("container ID is not a canonical UUID")),
        _ => Err(bad_state("container ID must be a string")),
    }
}

// ------------------------------------------------------------------ state view helper

/// The read access the container rules need: any project entity by ID, as its entity kind
/// and canonical body. The bus state view implements it in `fid-project-cmd`; read methods
/// only, shared references only.
pub trait ContainerView {
    /// The entity whose ID is the canonical text of `id`, if any: `(kind, body)`.
    fn entity(&self, id: &Uuid) -> Option<(&str, &CValue)>;
}

/// The container hierarchy as seen through a read-only [`ContainerView`].
#[derive(Clone, Copy)]
pub struct Hierarchy<'a> {
    view: &'a dyn ContainerView,
}

impl fmt::Debug for Hierarchy<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Hierarchy")
    }
}

impl<'a> Hierarchy<'a> {
    pub fn new(view: &'a dyn ContainerView) -> Hierarchy<'a> {
        Hierarchy { view }
    }

    /// The container with this ID. `Ok(None)` if no entity has the ID or the entity is not
    /// a container; `bad_state` if a container entity's body is malformed or disagrees with
    /// its entity kind or ID.
    pub fn get(&self, id: &Uuid) -> Result<Option<Container>, ContainerError> {
        let Some((kind, body)) = self.view.entity(id) else {
            return Ok(None);
        };
        let Some(kind) = ContainerKind::parse(kind) else {
            return Ok(None);
        };
        let c = Container::from_value(body)?;
        if c.kind() != kind || c.id() != *id {
            return Err(bad_state("container body disagrees with its entity kind or ID"));
        }
        Ok(Some(c))
    }

    /// The container with this ID and kind; `bad_reference` if it is missing or of another
    /// kind (`what` names the parameter).
    pub fn expect(&self, id: &Uuid, kind: ContainerKind, what: &str) -> Result<Container, ContainerError> {
        match self.get(id)? {
            Some(c) if c.kind() == kind => Ok(c),
            Some(c) => Err(ContainerError::BadReference(format!("{what} {id} is a {}, not a {kind}", c.kind()))),
            None => Err(ContainerError::BadReference(format!("{what} {id} is not a {kind} of this project"))),
        }
    }

    /// The container `id` and its ancestors, nearest first (a Run, its Plan, Revision and
    /// Part). `bad_state` if a link is missing or the chain is longer than the hierarchy.
    /// The view has no listing, so the hierarchy is navigated upwards only.
    pub fn parent_chain(&self, id: &Uuid) -> Result<Vec<Container>, ContainerError> {
        let mut out = Vec::new();
        let mut next = Some(*id);
        while let Some(at) = next {
            if out.len() >= ContainerKind::ALL.len() {
                return Err(bad_state("container parent chain is too long"));
            }
            let Some(c) = self.get(&at)? else {
                return Err(bad_state("container parent missing"));
            };
            next = c.parent();
            out.push(c);
        }
        Ok(out)
    }
}

impl IdIndex for Hierarchy<'_> {
    /// Any entity of the project, container or not, holds the ID.
    fn contains_id(&self, id: &Uuid) -> bool {
        self.view.entity(id).is_some()
    }
}

// ------------------------------------------------------------------ commands

type Params<'a> = &'a BTreeMap<String, CValue>;

fn allowed_keys(kind: ContainerKind) -> &'static [&'static str] {
    match kind {
        ContainerKind::Part => &["id", "label"],
        ContainerKind::Revision => &["id", "label", "part"],
        ContainerKind::Plan => &["id", "label", "revision"],
        ContainerKind::Run => &["id", "label", "plan", "revision"],
    }
}

fn reference(p: Params<'_>, key: &str) -> Result<Uuid, ContainerError> {
    match p.get(key) {
        Some(CValue::Str(s)) => {
            canonical_uuid(s).ok_or_else(|| ContainerError::Schema(format!("{key} must be a canonical lowercase UUID")))
        }
        Some(_) => Err(ContainerError::Schema(format!("{key} must be a string"))),
        None => Err(ContainerError::Schema(format!("{key} is required"))),
    }
}

/// The checked parent references of a create command.
enum Links {
    Part,
    Revision(Uuid),
    Plan(Uuid),
    Run(Uuid, Uuid),
}

/// Runs the create command of `kind` against `view` with the parameter object `params` for
/// `created_by`, drawing a new ID from `source` when no `id` is imported. Returns the new
/// container; writes nothing. A refused command draws nothing from `source`.
pub fn create(
    kind: ContainerKind,
    view: &dyn ContainerView,
    params: &CValue,
    created_by: &CreatedBy,
    namespace: Uuid,
    source: &mut dyn ObservationSource,
) -> Result<Container, ContainerError> {
    // 1. Shape.
    let CValue::Obj(p) = params else {
        return Err(schema("parameters must be an object"));
    };
    let allowed = allowed_keys(kind);
    if let Some(k) = p.keys().find(|k| !allowed.contains(&k.as_str())) {
        return Err(ContainerError::Schema(format!("unknown parameter {k:?}")));
    }
    let label_text = match p.get("label") {
        Some(CValue::Str(s)) => Some(s.as_str()),
        Some(_) => return Err(schema("label must be a string")),
        None if kind.label_required() => return Err(schema("label is required")),
        None => None,
    };
    let refs: Vec<Uuid> = match kind {
        ContainerKind::Part => Vec::new(),
        ContainerKind::Revision => vec![reference(p, "part")?],
        ContainerKind::Plan => vec![reference(p, "revision")?],
        ContainerKind::Run => vec![reference(p, "plan")?, reference(p, "revision")?],
    };
    let imported = match p.get("id") {
        None => None,
        Some(CValue::Str(s)) => Some(canonical_uuid(s).ok_or_else(|| schema("id must be a canonical lowercase UUID"))?),
        Some(_) => return Err(schema("id must be a string")),
    };
    if imported.is_some_and(|id| id == Uuid::NIL || id == Uuid::MAX) {
        return Err(schema("id must not be the nil or max UUID"));
    }
    // 2. Label.
    let label = label_text.map(Label::new).transpose()?;
    // 3. References, then 4. the Plan-Revision rule.
    let h = Hierarchy::new(view);
    let links = match (kind, refs.as_slice()) {
        (ContainerKind::Part, []) => Links::Part,
        (ContainerKind::Revision, [part]) => {
            h.expect(part, ContainerKind::Part, "part")?;
            Links::Revision(*part)
        }
        (ContainerKind::Plan, [revision]) => {
            h.expect(revision, ContainerKind::Revision, "revision")?;
            Links::Plan(*revision)
        }
        (ContainerKind::Run, [plan, revision]) => {
            let Container::Plan(pl) = h.expect(plan, ContainerKind::Plan, "plan")? else {
                return Err(bad_state("plan lookup returned another kind"));
            };
            h.expect(revision, ContainerKind::Revision, "revision")?;
            if pl.revision != *revision {
                return Err(ContainerError::Hierarchy(format!(
                    "plan {plan} belongs to revision {}, not {revision}",
                    pl.revision
                )));
            }
            Links::Run(*plan, *revision)
        }
        _ => return Err(bad_state("reference list does not match the kind")),
    };
    // 5. The ID: imported unchanged, or a fresh UUIDv7. Nothing is drawn before this point.
    let mut ids = IdService::new(namespace, source, &h);
    let id = match imported {
        Some(id) => ids.import_uuid(id)?,
        None => {
            let id = ids.new_v7()?;
            if h.contains_id(&id) {
                return Err(ContainerError::IdCollision(id));
            }
            id
        }
    };
    let created_by = created_by.clone();
    Ok(match (links, label) {
        (Links::Part, Some(label)) => Container::Part(Part { id, label, created_by }),
        (Links::Revision(part), Some(label)) => Container::Revision(Revision { id, part, label, created_by }),
        (Links::Plan(revision), label) => Container::Plan(Plan { id, revision, label, created_by }),
        (Links::Run(plan, revision), label) => Container::Run(Run { id, plan, revision, label, created_by }),
        (Links::Part | Links::Revision(_), None) => return Err(schema("label is required")),
    })
}
