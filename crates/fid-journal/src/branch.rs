//! Named branches: create, list, protection and co-editing opt-in (spec node
//! `CORE.JRN.BR.NAME`).
//!
//! The branch table is a projection of three events, never stored on its own:
//! `BranchCreated {name, from}`, `BranchProtected {name, cause}` and
//! `CoeditingSet {name, agent_session, allow}` ([`BranchEvent`]). [`Branches::apply`] folds
//! one event and refuses any event the planners below would never emit (a duplicate name, a
//! protection on a missing or already protected branch, a co-editing change that changes
//! nothing), so a tampered or replayed-out-of-order event log fails closed.
//!
//! Planners ([`plan_create`], [`plan_agent_branch`], [`plan_protect`], [`plan_coediting`],
//! [`release_protection`]) check a request against the current table and return the event to
//! commit, or a [`BranchError`] whose [`BranchError::code`] and [`BranchError::reason`] are
//! a code and reason of the bus error catalogue (v3). They take the caller as an [`Actor`]
//! that the bus derives from the call's principal; nothing in the parameters names the
//! caller.
//!
//! Names ([`BranchName`]): `[a-z0-9][a-z0-9/_.-]{0,99}` (REQ-002), ASCII only, so a name is
//! its own canonical bytes and has no Unicode look-alike; in addition no empty, `.` or `..`
//! path segment and no trailing `/`. Ownership follows from the name alone: a name in the
//! `agent/` namespace is an agent branch, every other name a human branch, and the name
//! `agent` itself is refused. `journal.create_branch@1` never creates an agent branch, and an
//! agent caller cannot create any branch through it: agent branches `agent/<client>/<n>` come
//! only from [`plan_agent_branch`], which the bus's routing module calls. There is no rename
//! and no delete, so a name is never reused, and nothing clears a protection (REQ-005).

use crate::entry::PrincipalKind;
use fid_canon::CValue;
use fid_types::{Digest, SideEffect};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// JSON Schema pattern of the grammar part of a branch name (REQ-002).
pub const NAME_PATTERN: &str = "^[a-z0-9][a-z0-9/_.-]{0,99}$";
/// Longest branch name, in bytes.
pub const MAX_NAME_BYTES: usize = 100;
/// The namespace of agent branches.
pub const AGENT_PREFIX: &str = "agent/";
/// The branch every project starts with.
pub const MAIN: &str = "main";
/// JSON Schema pattern of an agent session ID.
pub const SESSION_PATTERN: &str = "^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$";
/// JSON Schema pattern of an agent client name in `agent/<client>/<n>`.
pub const CLIENT_PATTERN: &str = "^[a-z0-9][a-z0-9_.-]{0,62}$";

/// Event types.
pub const BRANCH_CREATED: &str = "BranchCreated";
pub const BRANCH_PROTECTED: &str = "BranchProtected";
pub const COEDITING_SET: &str = "CoeditingSet";

/// Command IDs.
pub const CREATE_BRANCH: &str = "journal.create_branch";
pub const LIST_BRANCHES: &str = "journal.list_branches";
pub const PROTECT_BRANCH: &str = "journal.protect_branch";
pub const SET_BRANCH_COEDITING: &str = "journal.set_branch_coediting";

// ------------------------------------------------------------------ errors

/// Why a request or an event was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BranchError {
    /// The name breaks the grammar, exists already, is reserved for the routing module, or
    /// may not be used by this caller (`validation/bad_branch_name`).
    BadName(String),
    /// A parameter that is not a name is malformed (`validation/schema`).
    Schema(String),
    /// No branch has this name (`not_found/unknown_branch`).
    UnknownBranch(String),
    /// The source head is not a head this project knows (`not_found/unknown_entity`).
    UnknownHead(Digest),
    /// Only a human principal may run this W3 command (`policy/class_forbidden`).
    ClassForbidden(String),
    /// Only a human principal may run this human-only command (`policy/human_only`).
    HumanOnly(String),
    /// An event of the log contradicts the table (`internal/invariant_violated`).
    Tampered(String),
}

impl BranchError {
    /// The bus error code (catalogue v3).
    pub fn code(&self) -> &'static str {
        match self {
            BranchError::BadName(_) | BranchError::Schema(_) => "validation",
            BranchError::UnknownBranch(_) | BranchError::UnknownHead(_) => "not_found",
            BranchError::ClassForbidden(_) | BranchError::HumanOnly(_) => "policy",
            BranchError::Tampered(_) => "internal",
        }
    }

    /// The bus error reason (catalogue v3).
    pub fn reason(&self) -> &'static str {
        match self {
            BranchError::BadName(_) => "bad_branch_name",
            BranchError::Schema(_) => "schema",
            BranchError::UnknownBranch(_) => "unknown_branch",
            BranchError::UnknownHead(_) => "unknown_entity",
            BranchError::ClassForbidden(_) => "class_forbidden",
            BranchError::HumanOnly(_) => "human_only",
            BranchError::Tampered(_) => "invariant_violated",
        }
    }

    /// The detail text.
    pub fn detail(&self) -> String {
        match self {
            BranchError::BadName(d)
            | BranchError::Schema(d)
            | BranchError::ClassForbidden(d)
            | BranchError::HumanOnly(d)
            | BranchError::Tampered(d) => d.clone(),
            BranchError::UnknownBranch(n) => format!("no branch named {n:?}"),
            BranchError::UnknownHead(d) => format!("{d} is not a head of this project"),
        }
    }
}

impl fmt::Display for BranchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}: {}", self.code(), self.reason(), self.detail())
    }
}

impl std::error::Error for BranchError {}

fn bad(why: impl Into<String>) -> BranchError {
    BranchError::BadName(why.into())
}

fn tampered(why: impl Into<String>) -> BranchError {
    BranchError::Tampered(why.into())
}

// ------------------------------------------------------------------ names

/// Who owns a branch, from its name alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Owner {
    Human,
    Agent,
}

impl Owner {
    pub fn as_str(self) -> &'static str {
        match self {
            Owner::Human => "human",
            Owner::Agent => "agent",
        }
    }
}

/// A checked branch name. Its bytes are its canonical form.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BranchName(String);

impl BranchName {
    /// Checks `s` against the grammar and the path rules (module docs). The `agent/`
    /// namespace is allowed here; who may create such a name is the planners' business.
    pub fn parse(s: &str) -> Result<BranchName, BranchError> {
        let b = s.as_bytes();
        if b.is_empty() || b.len() > MAX_NAME_BYTES {
            return Err(bad(format!("a branch name has 1 to {MAX_NAME_BYTES} bytes, this one {}", b.len())));
        }
        let first_ok = b[0].is_ascii_lowercase() || b[0].is_ascii_digit();
        let rest_ok = b.iter().all(|&c| c.is_ascii_lowercase() || c.is_ascii_digit() || b"/_.-".contains(&c));
        if !first_ok || !rest_ok {
            return Err(bad(format!(
                "{:?} does not match {NAME_PATTERN} (lowercase ASCII letters, digits and / _ . - only)",
                truncate(s)
            )));
        }
        if s.ends_with('/') || s.split('/').any(|seg| seg.is_empty() || seg == "." || seg == "..") {
            return Err(bad(format!("{s:?} has an empty, \".\" or \"..\" path segment or ends with /")));
        }
        if s == "agent" {
            return Err(bad("\"agent\" is reserved: it reads as the agent branch namespace"));
        }
        Ok(BranchName(s.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// `Agent` for a name in the `agent/` namespace, else `Human`.
    pub fn owner(&self) -> Owner {
        if self.0.starts_with(AGENT_PREFIX) {
            Owner::Agent
        } else {
            Owner::Human
        }
    }
}

impl fmt::Display for BranchName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// At most 120 characters of untrusted text for an error detail.
fn truncate(s: &str) -> String {
    s.chars().take(120).collect()
}

fn matches_ascii(s: &str, max: usize, first: fn(u8) -> bool, rest: fn(u8) -> bool) -> bool {
    let b = s.as_bytes();
    !b.is_empty() && b.len() <= max && first(b[0]) && b.iter().all(|&c| rest(c))
}

/// An agent session ID ([`SESSION_PATTERN`]).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SessionId(String);

impl SessionId {
    pub fn parse(s: &str) -> Result<SessionId, BranchError> {
        let ok = matches_ascii(
            s,
            128,
            |c| c.is_ascii_alphanumeric(),
            |c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b':' | b'-'),
        );
        if ok {
            Ok(SessionId(s.to_string()))
        } else {
            Err(BranchError::Schema(format!("agent session {:?} does not match {SESSION_PATTERN}", truncate(s))))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

// ------------------------------------------------------------------ table

/// One branch of the table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Branch {
    pub name: BranchName,
    /// The current head.
    pub head: Digest,
    /// The head it was created at; `None` for `main`.
    pub created_from: Option<Digest>,
    pub protected: bool,
    /// Agent sessions a human allowed to commit to this branch directly.
    pub coediting: BTreeSet<SessionId>,
}

impl Branch {
    pub fn owner(&self) -> Owner {
        self.name.owner()
    }

    /// `{name, head, created_from, protected, owner, coediting}` (the `BranchInfo` of the
    /// command results).
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("name".into(), CValue::Str(self.name.0.clone()));
        o.insert("head".into(), CValue::Str(self.head.as_str().into()));
        o.insert("created_from".into(), self.created_from.map_or(CValue::Null, |d| CValue::Str(d.as_str().into())));
        o.insert("protected".into(), CValue::Bool(self.protected));
        o.insert("owner".into(), CValue::Str(self.owner().as_str().into()));
        o.insert("coediting".into(), CValue::Arr(self.coediting.iter().map(|s| CValue::Str(s.0.clone())).collect()));
        CValue::Obj(o)
    }
}

/// Why a branch was protected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtectCause {
    /// `journal.protect_branch@1`.
    Manual,
    /// The first W3 release record on `main` (REQ-006).
    Release,
}

impl ProtectCause {
    pub fn as_str(self) -> &'static str {
        match self {
            ProtectCause::Manual => "manual",
            ProtectCause::Release => "release",
        }
    }
}

/// The three branch events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BranchEvent {
    Created { name: BranchName, from: Digest },
    Protected { name: BranchName, cause: ProtectCause },
    Coediting { name: BranchName, session: SessionId, allow: bool },
}

fn obj<const N: usize>(pairs: [(&str, CValue); N]) -> CValue {
    CValue::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

impl BranchEvent {
    /// The event type.
    pub fn type_id(&self) -> &'static str {
        match self {
            BranchEvent::Created { .. } => BRANCH_CREATED,
            BranchEvent::Protected { .. } => BRANCH_PROTECTED,
            BranchEvent::Coediting { .. } => COEDITING_SET,
        }
    }

    /// The event body.
    pub fn body(&self) -> CValue {
        let s = |v: &str| CValue::Str(v.to_string());
        match self {
            BranchEvent::Created { name, from } => obj([("from", s(from.as_str())), ("name", s(name.as_str()))]),
            BranchEvent::Protected { name, cause } => obj([("cause", s(cause.as_str())), ("name", s(name.as_str()))]),
            BranchEvent::Coediting { name, session, allow } => obj([
                ("agent_session", s(session.as_str())),
                ("allow", CValue::Bool(*allow)),
                ("name", s(name.as_str())),
            ]),
        }
    }

    /// Parses a stored event strictly: a known type, exactly its keys, every value well
    /// formed. Anything else is [`BranchError::Tampered`].
    pub fn from_parts(type_id: &str, body: &CValue) -> Result<BranchEvent, BranchError> {
        let CValue::Obj(o) = body else { return Err(tampered(format!("{type_id} body is not an object"))) };
        let keys: Vec<&str> = o.keys().map(String::as_str).collect();
        let want: &[&str] = match type_id {
            BRANCH_CREATED => &["from", "name"],
            BRANCH_PROTECTED => &["cause", "name"],
            COEDITING_SET => &["agent_session", "allow", "name"],
            other => return Err(tampered(format!("unknown branch event type {:?}", truncate(other)))),
        };
        if keys != want {
            return Err(tampered(format!("{type_id} has keys {keys:?}, expected {want:?}")));
        }
        let text = |k: &str| match o.get(k) {
            Some(CValue::Str(s)) => Ok(s.as_str()),
            _ => Err(tampered(format!("{type_id}.{k} is not a string"))),
        };
        let name = BranchName::parse(text("name")?).map_err(|e| tampered(format!("{type_id}.name: {}", e.detail())))?;
        Ok(match type_id {
            BRANCH_CREATED => {
                let from = Digest::parse(text("from")?).map_err(|_| tampered("BranchCreated.from is not a digest"))?;
                BranchEvent::Created { name, from }
            }
            BRANCH_PROTECTED => {
                let cause = match text("cause")? {
                    "manual" => ProtectCause::Manual,
                    "release" => ProtectCause::Release,
                    _ => return Err(tampered("BranchProtected.cause is not manual or release")),
                };
                BranchEvent::Protected { name, cause }
            }
            _ => {
                let session =
                    SessionId::parse(text("agent_session")?).map_err(|e| tampered(format!("CoeditingSet: {e}")))?;
                let Some(CValue::Bool(allow)) = o.get("allow") else {
                    return Err(tampered("CoeditingSet.allow is not a boolean"));
                };
                BranchEvent::Coediting { name, session, allow: *allow }
            }
        })
    }
}

/// The branch table, keyed (and so sorted) by name bytes.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Branches {
    table: BTreeMap<BranchName, Branch>,
}

impl Branches {
    /// The table of a project whose `main` is at `main_head`, before any branch event.
    pub fn genesis(main_head: Digest) -> Branches {
        let name = BranchName(MAIN.to_string());
        let main = Branch {
            name: name.clone(),
            head: main_head,
            created_from: None,
            protected: false,
            coediting: BTreeSet::new(),
        };
        Branches { table: BTreeMap::from([(name, main)]) }
    }

    /// Rebuilds the table from `main`'s first head and the branch events in commit order.
    pub fn replay<'a>(
        main_head: Digest,
        events: impl IntoIterator<Item = &'a BranchEvent>,
    ) -> Result<Branches, BranchError> {
        let mut b = Branches::genesis(main_head);
        for e in events {
            b.apply(e)?;
        }
        Ok(b)
    }

    /// Folds one committed event. Refuses an event the planners would not emit.
    pub fn apply(&mut self, event: &BranchEvent) -> Result<(), BranchError> {
        match event {
            BranchEvent::Created { name, from } => {
                if self.table.contains_key(name) {
                    return Err(tampered(format!("BranchCreated for existing branch {name}")));
                }
                let b = Branch {
                    name: name.clone(),
                    head: *from,
                    created_from: Some(*from),
                    protected: false,
                    coediting: BTreeSet::new(),
                };
                self.table.insert(name.clone(), b);
            }
            BranchEvent::Protected { name, cause } => {
                let b =
                    self.table.get_mut(name).ok_or_else(|| tampered(format!("BranchProtected for unknown {name}")))?;
                if b.protected {
                    return Err(tampered(format!("BranchProtected for already protected {name}")));
                }
                if *cause == ProtectCause::Release && name.as_str() != MAIN {
                    return Err(tampered(format!("release protection of {name}, not main")));
                }
                b.protected = true;
            }
            BranchEvent::Coediting { name, session, allow } => {
                let b = self.table.get_mut(name).ok_or_else(|| tampered(format!("CoeditingSet for unknown {name}")))?;
                if b.owner() == Owner::Agent {
                    return Err(tampered(format!("CoeditingSet on agent branch {name}")));
                }
                let changed = if *allow { b.coediting.insert(session.clone()) } else { b.coediting.remove(session) };
                if !changed {
                    return Err(tampered(format!("CoeditingSet on {name} changes nothing")));
                }
            }
        }
        Ok(())
    }

    /// Moves a branch head (a commit on that branch, `CORE.JRN.CHAIN.APPEND`).
    pub fn set_head(&mut self, name: &BranchName, head: Digest) -> Result<(), BranchError> {
        let b = self.table.get_mut(name).ok_or_else(|| BranchError::UnknownBranch(name.to_string()))?;
        b.head = head;
        Ok(())
    }

    /// Every branch, sorted by name (REQ-003).
    pub fn list(&self) -> Vec<&Branch> {
        self.table.values().collect()
    }

    pub fn get(&self, name: &str) -> Option<&Branch> {
        BranchName::parse(name).ok().and_then(|n| self.table.get(&n))
    }

    pub fn head(&self, name: &str) -> Option<Digest> {
        self.get(name).map(|b| b.head)
    }

    pub fn is_protected(&self, name: &str) -> Option<bool> {
        self.get(name).map(|b| b.protected)
    }

    /// The agent sessions allowed to commit to `name` directly.
    pub fn coediting(&self, name: &str) -> Option<&BTreeSet<SessionId>> {
        self.get(name).map(|b| &b.coediting)
    }

    /// Whether `head` is the head or the creation point of some branch.
    pub fn knows_head(&self, head: &Digest) -> bool {
        self.table.values().any(|b| b.head == *head || b.created_from == Some(*head))
    }

    /// `{"branches": [BranchInfo...]}` sorted by name: the `journal.list_branches@1` result.
    pub fn to_value(&self) -> CValue {
        obj([("branches", CValue::Arr(self.table.values().map(Branch::to_value).collect()))])
    }

    fn existing(&self, name: &str) -> Result<&Branch, BranchError> {
        let n = BranchName::parse(name).map_err(|_| BranchError::UnknownBranch(truncate(name)))?;
        self.table.get(&n).ok_or_else(|| BranchError::UnknownBranch(n.0.clone()))
    }
}

// ------------------------------------------------------------------ planners

/// The caller, as the bus derives it from the call's principal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Actor {
    Human,
    Agent,
    /// Any other principal kind (script, service, device, system).
    Other,
}

impl Actor {
    /// The actor of a principal kind. The bus passes the kind of the principal it derived from
    /// the channel (`HandlerCtx::principal`); parameters never name the caller.
    pub fn of(kind: PrincipalKind) -> Actor {
        match kind {
            PrincipalKind::Human => Actor::Human,
            PrincipalKind::Agent => Actor::Agent,
            PrincipalKind::Script | PrincipalKind::Service | PrincipalKind::Device | PrincipalKind::System => {
                Actor::Other
            }
        }
    }
}

/// `journal.create_branch@1` (REQ-001, REQ-002): a human branch at `from`.
pub fn plan_create(
    b: &Branches,
    actor: Actor,
    name: &str,
    from: &str,
    is_head: &dyn Fn(&Digest) -> bool,
) -> Result<BranchEvent, BranchError> {
    let name = BranchName::parse(name)?;
    if name.owner() == Owner::Agent {
        return Err(bad(format!("{name}: the agent/ namespace is reserved for the bus routing module")));
    }
    if actor == Actor::Agent {
        return Err(bad(format!(
            "{name}: an agent gets branches only from the bus routing module (agent/<client>/<n>)"
        )));
    }
    if b.table.contains_key(&name) {
        return Err(bad(format!("{name} exists already; branch names are never reused")));
    }
    let from = Digest::parse(from).map_err(|_| BranchError::Schema("from is not a sha256: digest".into()))?;
    if !(b.knows_head(&from) || is_head(&from)) {
        return Err(BranchError::UnknownHead(from));
    }
    Ok(BranchEvent::Created { name, from })
}

/// The agent branch `agent/<client>/<n>` at `from`, for the bus routing module
/// (`CORE.BUS.ROUTE`); the name is built here, never taken from a caller.
pub fn plan_agent_branch(b: &Branches, client: &str, n: u64, from: Digest) -> Result<BranchEvent, BranchError> {
    let ok = matches_ascii(
        client,
        63,
        |c| c.is_ascii_lowercase() || c.is_ascii_digit(),
        |c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'_' | b'.' | b'-'),
    );
    if !ok || client == "." || client == ".." {
        return Err(bad(format!("agent client {:?} does not match {CLIENT_PATTERN}", truncate(client))));
    }
    let name = BranchName::parse(&format!("{AGENT_PREFIX}{client}/{n}"))?;
    if b.table.contains_key(&name) {
        return Err(bad(format!("{name} exists already; branch names are never reused")));
    }
    Ok(BranchEvent::Created { name, from })
}

/// `journal.protect_branch@1` (REQ-004): `None` if the branch is protected already.
pub fn plan_protect(b: &Branches, actor: Actor, name: &str) -> Result<Option<BranchEvent>, BranchError> {
    if actor != Actor::Human {
        return Err(BranchError::ClassForbidden("protecting a branch (W3) needs a human principal".into()));
    }
    let br = b.existing(name)?;
    if br.protected {
        return Ok(None);
    }
    Ok(Some(BranchEvent::Protected { name: br.name.clone(), cause: ProtectCause::Manual }))
}

/// `journal.set_branch_coediting@1` (REQ-007): `None` if nothing would change.
pub fn plan_coediting(
    b: &Branches,
    actor: Actor,
    name: &str,
    session: &str,
    allow: bool,
) -> Result<Option<BranchEvent>, BranchError> {
    if actor != Actor::Human {
        return Err(BranchError::HumanOnly("a co-editing opt-in is recorded by a human only".into()));
    }
    let br = b.existing(name)?;
    if br.owner() == Owner::Agent {
        return Err(bad(format!("{}: co-editing opt-ins are for human branches", br.name)));
    }
    let session = SessionId::parse(session)?;
    if br.coediting.contains(&session) == allow {
        return Ok(None);
    }
    Ok(Some(BranchEvent::Coediting { name: br.name.clone(), session, allow }))
}

/// REQ-006: the event that protects `main` in the same commit as the first W3 record on it,
/// or `None` (another branch, another class, or `main` already protected).
pub fn release_protection(b: &Branches, branch: &str, class: SideEffect) -> Option<BranchEvent> {
    if branch != MAIN || class != SideEffect::W3 {
        return None;
    }
    let main = b.get(MAIN)?;
    (!main.protected).then(|| BranchEvent::Protected { name: main.name.clone(), cause: ProtectCause::Release })
}

#[cfg(feature = "store")]
pub use defs::command_defs;

#[cfg(feature = "store")]
mod defs;
