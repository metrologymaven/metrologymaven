//! The project-local defaults-profile store, the per-characteristic profile bindings, and
//! the two W2 commands that change them.
//!
//! **Branches (REQ-006, CONSTITUTION §3 W2 and C5).** A human principal writes to the
//! working branch. Any other principal (agent or script) writes to the agent branch: its
//! profiles carry its principal as provenance and are not selectable on the working branch
//! until a human adopts them ([`DprofState::adopt`]); its re-points go to an agent-branch
//! overlay of the bindings and leave the working bindings untouched. The agent branch sees
//! the working branch's profiles too.
//!
//! **Uniqueness (per branch, C5).** One digest per (dialect, edition, version) on each
//! branch: a version taken on the same branch with another digest is `profile_immutable`,
//! and an agent publish never blocks or changes anything on the working branch. An
//! identical re-publish on the same branch returns the stored profile unchanged and
//! journals nothing. Adoption copies an agent-branch profile onto the working branch
//! (provenance kept) and is refused with `adopt_conflict` when the working branch already
//! holds that version with another digest.

use super::event::{Change, DprofEvent, PublishEvent, RepointEvent};
use super::ident::{CharId, ProfileId, ProfileRef};
use super::profile::{profile_from_value, KeyVocabulary, ProfileDoc};
use super::{DprofError, MAX_FILE_PARAM_LEN};
use fid_canon::CValue;
use fid_io_reg::Principal;
use std::collections::{BTreeMap, BTreeSet};

/// Most profiles one store holds.
pub const MAX_PROFILES: usize = 65_536;
/// Most bindings per branch.
pub const MAX_BINDINGS: usize = 1_000_000;
/// Most characteristics one re-point may name.
pub const MAX_REPOINT_CHARS: usize = 10_000;
/// `schema` value of a store snapshot.
pub const STORE_SCHEMA: &str = "fid.dprof-store/1";
/// Largest snapshot accepted, in bytes.
pub const MAX_STORE_BYTES: usize = fid_canon::MAX_INPUT_BYTES;
/// Deepest snapshot nesting: document, `profiles`, entry, profile document, `keys`, list.
const MAX_STORE_DEPTH: usize = 6;

/// The branch a principal writes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Branch {
    /// The working branch.
    Working,
    /// The agent branch.
    Agent,
}

impl Branch {
    /// Wire name: `working` or `agent`.
    pub fn as_str(self) -> &'static str {
        match self {
            Branch::Working => "working",
            Branch::Agent => "agent",
        }
    }

    /// Parses the wire name.
    pub fn parse(s: &str) -> Option<Branch> {
        match s {
            "working" => Some(Branch::Working),
            "agent" => Some(Branch::Agent),
            _ => None,
        }
    }

    /// The branch `p` writes to: humans the working branch, everyone else the agent branch.
    pub fn for_principal(p: Principal) -> Branch {
        match p {
            Principal::Human => Branch::Working,
            Principal::Agent | Principal::Script => Branch::Agent,
        }
    }
}

/// Parses a principal wire name (`human`, `agent`, `script`).
pub fn parse_principal(s: &str) -> Option<Principal> {
    match s {
        "human" => Some(Principal::Human),
        "agent" => Some(Principal::Agent),
        "script" => Some(Principal::Script),
        _ => None,
    }
}

/// One stored profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredProfile {
    /// The profile, canonical bytes and digest.
    pub doc: ProfileDoc,
    /// The branch it was published on.
    pub branch: Branch,
    /// Provenance: the principal that published it.
    pub author: Principal,
    /// Whether this working-branch entry came from a human adopting an agent-branch
    /// profile (always `false` on the agent branch).
    pub adopted: bool,
}

impl StoredProfile {
    /// Whether a characteristic on `branch` may select this profile: the agent branch sees
    /// both branches, the working branch only its own entries.
    pub fn selectable_on(&self, branch: Branch) -> bool {
        match branch {
            Branch::Agent => true,
            Branch::Working => self.branch == Branch::Working,
        }
    }

    /// The full reference.
    pub fn reference(&self) -> ProfileRef {
        self.doc.reference()
    }
}

/// Whether a publish created a profile or found the identical one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublishStatus {
    /// Newly stored (or would be, for a dry run).
    Created,
    /// The identical profile already existed; nothing changed.
    Existing,
}

impl PublishStatus {
    /// Wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            PublishStatus::Created => "created",
            PublishStatus::Existing => "existing",
        }
    }
}

/// The result of `gdt.publish_defaults_profile@1`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Published {
    /// The profile reference.
    pub reference: ProfileRef,
    /// Created or existing.
    pub status: PublishStatus,
    /// The branch the profile lives on.
    pub branch: Branch,
    /// Whether this was a dry run.
    pub dry_run: bool,
    /// The journal event; `None` for an existing profile or a dry run.
    pub event: Option<DprofEvent>,
}

impl Published {
    /// The `--json` / MCP result.
    pub fn to_cvalue(&self) -> CValue {
        let mut m = BTreeMap::new();
        m.insert("ref".to_owned(), CValue::Str(self.reference.to_string()));
        m.insert("status".to_owned(), CValue::Str(self.status.as_str().to_owned()));
        m.insert("branch".to_owned(), CValue::Str(self.branch.as_str().to_owned()));
        m.insert("dry_run".to_owned(), CValue::Bool(self.dry_run));
        CValue::Obj(m)
    }
}

/// The result of `gdt.repoint_defaults_profile@1`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repointed {
    /// One entry per named characteristic, sorted by characteristic ID.
    pub changes: Vec<Change>,
    /// The branch the bindings were changed on.
    pub branch: Branch,
    /// Whether this was a dry run.
    pub dry_run: bool,
    /// The journal event; `None` for a dry run.
    pub event: Option<DprofEvent>,
}

impl Repointed {
    /// The `--json` / MCP result.
    pub fn to_cvalue(&self) -> CValue {
        let mut m = BTreeMap::new();
        m.insert("changes".to_owned(), CValue::Arr(self.changes.iter().map(Change::to_cvalue).collect()));
        m.insert("branch".to_owned(), CValue::Str(self.branch.as_str().to_owned()));
        m.insert("dry_run".to_owned(), CValue::Bool(self.dry_run));
        CValue::Obj(m)
    }
}

/// Profiles plus per-characteristic bindings on the working branch and the agent overlay.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DprofState {
    profiles: BTreeMap<(Branch, ProfileId), StoredProfile>,
    working: BTreeMap<CharId, ProfileRef>,
    agent: BTreeMap<CharId, ProfileRef>,
}

fn bad_request(reason: &'static str) -> DprofError {
    DprofError::BadRequest { reason }
}

fn store_err(reason: &'static str) -> DprofError {
    DprofError::Store { reason }
}

impl DprofState {
    /// An empty store.
    pub fn new() -> DprofState {
        DprofState::default()
    }

    /// Number of stored profiles.
    pub fn profile_count(&self) -> usize {
        self.profiles.len()
    }

    /// Every stored profile, working branch first, then by (dialect, edition, version).
    pub fn profiles(&self) -> impl Iterator<Item = &StoredProfile> {
        self.profiles.values()
    }

    /// The profile stored on `branch` with this dialect, edition and version.
    pub fn get(&self, branch: Branch, id: &ProfileId) -> Option<&StoredProfile> {
        self.profiles.get(&(branch, id.clone()))
    }

    /// Resolves a reference for a characteristic on `branch`: a profile with exactly this
    /// ID and digest must be selectable there (the working branch looks at its own
    /// entries; the agent branch at its own, then the working branch's), else
    /// `defaults_profile_unknown`. This is the check `gdt.set_governing_spec@1`
    /// (`GDT.CHAR.SPEC`) runs.
    pub fn resolve(&self, branch: Branch, r: &ProfileRef) -> Result<&StoredProfile, DprofError> {
        let found = |b: Branch| self.profiles.get(&(b, r.id.clone())).filter(|p| p.doc.digest == r.digest);
        let hit = match branch {
            Branch::Working => found(Branch::Working),
            Branch::Agent => found(Branch::Agent).or_else(|| found(Branch::Working)),
        };
        hit.ok_or_else(|| DprofError::unknown_ref(&r.to_string()))
    }

    /// Parses `text` and resolves it on `branch` (`bad_profile_ref`, then as [`Self::resolve`]).
    pub fn resolve_text(&self, branch: Branch, text: &str) -> Result<&StoredProfile, DprofError> {
        let r = ProfileRef::parse(text).ok_or(DprofError::BadProfileRef)?;
        self.resolve(branch, &r)
    }

    /// The profile a characteristic references as seen from `branch` (the agent branch sees
    /// its overlay first, then the working binding).
    pub fn binding(&self, branch: Branch, c: &CharId) -> Option<&ProfileRef> {
        match branch {
            Branch::Working => self.working.get(c),
            Branch::Agent => self.agent.get(c).or_else(|| self.working.get(c)),
        }
    }

    /// Binds a new characteristic to a profile (the hook `GDT.CHAR.SPEC` uses when a
    /// characteristic is created; that command journals it). The profile must resolve on
    /// the principal's branch and its dialect must equal `dialect`.
    pub fn bind(
        &mut self,
        principal: Principal,
        c: &CharId,
        dialect: crate::dmt_flow::roster::Dialect,
        r: &ProfileRef,
    ) -> Result<(), DprofError> {
        let branch = Branch::for_principal(principal);
        self.resolve(branch, r)?;
        if r.dialect() != dialect {
            return Err(DprofError::DialectMismatch { char: c.as_str().to_owned() });
        }
        if self.binding(branch, c).is_some() {
            return Err(DprofError::CharExists { char: c.as_str().to_owned() });
        }
        let map = match branch {
            Branch::Working => &mut self.working,
            Branch::Agent => &mut self.agent,
        };
        if map.len() >= MAX_BINDINGS {
            return Err(store_err("too_many_bindings"));
        }
        map.insert(c.clone(), r.clone());
        Ok(())
    }

    /// Sets (or replaces) the binding of `c` on `branch`'s own map. Used by
    /// `GDT.CHAR.SPEC` (`chars::spec`), which keeps the bindings a mirror of the
    /// governing specs: after its own checks, when a governing spec is set or inherited,
    /// and when a project is loaded. The caller has already resolved `r` on `branch`.
    pub(crate) fn set_binding(&mut self, branch: Branch, c: &CharId, r: &ProfileRef) -> Result<(), DprofError> {
        let map = match branch {
            Branch::Working => &mut self.working,
            Branch::Agent => &mut self.agent,
        };
        if !map.contains_key(c) && map.len() >= MAX_BINDINGS {
            return Err(store_err("too_many_bindings"));
        }
        map.insert(c.clone(), r.clone());
        Ok(())
    }

    /// The bindings held on `branch` itself (no fallback from the agent overlay to the
    /// working branch).
    pub(crate) fn own_bindings(&self, branch: Branch) -> &BTreeMap<CharId, ProfileRef> {
        match branch {
            Branch::Working => &self.working,
            Branch::Agent => &self.agent,
        }
    }

    /// `gdt.publish_defaults_profile@1` on an already parsed profile. `file` is the
    /// command's `file` parameter as given (recorded in the event). Never changes any
    /// binding (REQ-004).
    pub fn publish(
        &mut self,
        doc: ProfileDoc,
        principal: Principal,
        file: &str,
        dry_run: bool,
    ) -> Result<Published, DprofError> {
        if file.is_empty() || file.len() > MAX_FILE_PARAM_LEN {
            return Err(bad_request("bad_file"));
        }
        let id = doc.profile.id();
        let reference = doc.reference();
        let branch = Branch::for_principal(principal);
        if let Some(existing) = self.profiles.get(&(branch, id.clone())) {
            if existing.doc.digest != doc.digest {
                return Err(DprofError::ProfileImmutable { id: id.to_string() });
            }
            return Ok(Published {
                reference,
                status: PublishStatus::Existing,
                branch: existing.branch,
                dry_run,
                event: None,
            });
        }
        if self.profiles.len() >= MAX_PROFILES {
            return Err(store_err("too_many_profiles"));
        }
        if dry_run {
            return Ok(Published { reference, status: PublishStatus::Created, branch, dry_run, event: None });
        }
        let event = DprofEvent::Publish(PublishEvent {
            file: file.to_owned(),
            principal,
            branch,
            reference: reference.clone(),
        });
        self.profiles.insert((branch, id), StoredProfile { doc, branch, author: principal, adopted: false });
        Ok(Published { reference, status: PublishStatus::Created, branch, dry_run, event: Some(event) })
    }

    /// `gdt.repoint_defaults_profile@1`: moves the named characteristics to `to`. All
    /// checks run before any change, so a refusal changes nothing. Order of checks: the
    /// list (`bad_request`: empty, too long, bad ID, duplicate), the reference
    /// (`bad_profile_ref`, `defaults_profile_unknown` on the principal's branch), each
    /// characteristic in ID order (`char_unknown`, then `dialect_mismatch`). The event
    /// lists every named characteristic with its old and new reference, sorted by ID,
    /// including any already on `to`.
    pub fn repoint(
        &mut self,
        principal: Principal,
        chars: &[&str],
        to: &str,
        dry_run: bool,
    ) -> Result<Repointed, DprofError> {
        if chars.is_empty() {
            return Err(bad_request("no_chars"));
        }
        if chars.len() > MAX_REPOINT_CHARS {
            return Err(bad_request("too_many_chars"));
        }
        let mut ids = BTreeSet::new();
        for c in chars {
            let id = CharId::parse(c).ok_or_else(|| bad_request("bad_char_id"))?;
            if !ids.insert(id) {
                return Err(bad_request("duplicate_char"));
            }
        }
        let branch = Branch::for_principal(principal);
        let target = self.resolve_text(branch, to)?.reference();
        let mut changes = Vec::with_capacity(ids.len());
        for c in &ids {
            let old = self.binding(branch, c).ok_or_else(|| DprofError::CharUnknown { char: c.as_str().to_owned() })?;
            if old.dialect() != target.dialect() {
                return Err(DprofError::DialectMismatch { char: c.as_str().to_owned() });
            }
            changes.push(Change { char: c.clone(), old: old.clone(), new: target.clone() });
        }
        if dry_run {
            return Ok(Repointed { changes, branch, dry_run, event: None });
        }
        let map = match branch {
            Branch::Working => &mut self.working,
            Branch::Agent => &mut self.agent,
        };
        let added = changes.iter().filter(|ch| !map.contains_key(&ch.char)).count();
        if map.len().saturating_add(added) > MAX_BINDINGS {
            return Err(store_err("too_many_bindings"));
        }
        for ch in &changes {
            map.insert(ch.char.clone(), ch.new.clone());
        }
        let event = DprofEvent::Repoint(RepointEvent { principal, branch, to: target, changes: changes.clone() });
        Ok(Repointed { changes, branch, dry_run, event: Some(event) })
    }

    /// Adopts an agent-branch profile so working-branch characteristics may select it: the
    /// profile is copied onto the working branch with its agent provenance and
    /// `adopted: true`. Checks, in order: human only (`human_only`); the reference names
    /// an agent-branch profile with that digest (`defaults_profile_unknown`); the working
    /// branch holds that version with another digest (`adopt_conflict`). If the working
    /// branch already holds the identical profile, nothing changes. Under D-037 this runs
    /// as an item of the H2 release batch, which journals it.
    pub fn adopt(&mut self, principal: Principal, r: &ProfileRef) -> Result<(), DprofError> {
        if principal != Principal::Human {
            return Err(DprofError::HumanOnly);
        }
        let agent = match self.profiles.get(&(Branch::Agent, r.id.clone())) {
            Some(p) if p.doc.digest == r.digest => p.clone(),
            _ => return Err(DprofError::unknown_ref(&r.to_string())),
        };
        if let Some(w) = self.profiles.get(&(Branch::Working, r.id.clone())) {
            if w.doc.digest == r.digest {
                return Ok(());
            }
            return Err(DprofError::AdoptConflict { id: r.id.to_string() });
        }
        if self.profiles.len() >= MAX_PROFILES {
            return Err(store_err("too_many_profiles"));
        }
        let copy = StoredProfile { doc: agent.doc, branch: Branch::Working, author: agent.author, adopted: true };
        self.profiles.insert((Branch::Working, r.id.clone()), copy);
        Ok(())
    }

    // ------------------------------------------------------------------ snapshot

    /// The store as canonical JSON (`fid.dprof-store/1`):
    ///
    /// ```json
    /// { "schema": "fid.dprof-store/1",
    ///   "profiles": [ { "ref": "dp:..", "branch": "working", "author": "human",
    ///                   "adopted": false, "profile": { ..the profile document.. } } ],
    ///   "bindings": { "working": { "<char>": "dp:.." }, "agent": { } } }
    /// ```
    pub fn to_canonical_bytes(&self) -> Result<Vec<u8>, DprofError> {
        self.to_canonical_bytes_capped(MAX_STORE_BYTES)
    }

    /// As [`Self::to_canonical_bytes`], refusing (`store_invalid{too_large}`) output longer
    /// than `cap` bytes; `cap` is clamped to [`MAX_STORE_BYTES`], so every snapshot written
    /// can be loaded by [`Self::from_canonical_bytes`].
    pub fn to_canonical_bytes_capped(&self, cap: usize) -> Result<Vec<u8>, DprofError> {
        let cap = cap.min(MAX_STORE_BYTES);
        let mut profiles = Vec::with_capacity(self.profiles.len());
        for p in self.profiles.values() {
            let doc = fid_canon::parse(&p.doc.canonical).map_err(|_| store_err("profile"))?;
            let mut m = BTreeMap::new();
            m.insert("ref".to_owned(), CValue::Str(p.reference().to_string()));
            m.insert("branch".to_owned(), CValue::Str(p.branch.as_str().to_owned()));
            m.insert("author".to_owned(), CValue::Str(p.author.as_str().to_owned()));
            m.insert("adopted".to_owned(), CValue::Bool(p.adopted));
            m.insert("profile".to_owned(), doc);
            profiles.push(CValue::Obj(m));
        }
        let side = |b: &BTreeMap<CharId, ProfileRef>| {
            CValue::Obj(b.iter().map(|(c, r)| (c.as_str().to_owned(), CValue::Str(r.to_string()))).collect())
        };
        let mut bindings = BTreeMap::new();
        bindings.insert("working".to_owned(), side(&self.working));
        bindings.insert("agent".to_owned(), side(&self.agent));
        let mut top = BTreeMap::new();
        top.insert("schema".to_owned(), CValue::Str(STORE_SCHEMA.to_owned()));
        top.insert("profiles".to_owned(), CValue::Arr(profiles));
        top.insert("bindings".to_owned(), CValue::Obj(bindings));
        let out = fid_canon::to_canonical_bytes(&CValue::Obj(top)).map_err(|_| store_err("encode"))?;
        if out.len() > cap {
            return Err(store_err("too_large"));
        }
        Ok(out)
    }

    /// Loads a snapshot strictly: every field present and no other, no non-integer number
    /// literal, each profile re-parsed against `vocab` with its digest re-checked,
    /// provenance consistent with its branch, no repeated (branch, profile ID), each
    /// binding resolvable on its branch, all limits held. Fails `store_invalid{reason}`,
    /// or the profile's own error.
    pub fn from_canonical_bytes(bytes: &[u8], vocab: &KeyVocabulary) -> Result<DprofState, DprofError> {
        if bytes.len() > MAX_STORE_BYTES {
            return Err(store_err("too_large"));
        }
        let v = fid_canon::parse_with_limits(bytes, MAX_STORE_BYTES, MAX_STORE_DEPTH).map_err(|_| store_err("json"))?;
        if !super::profile::integer_literals_only(bytes) {
            return Err(store_err("bad_number"));
        }
        let top = obj(&v, &["bindings", "profiles", "schema"])?;
        if str_of(top, "schema")? != STORE_SCHEMA {
            return Err(store_err("bad_schema"));
        }
        let mut state = DprofState::new();
        let list = match top.get("profiles") {
            Some(CValue::Arr(a)) => a,
            _ => return Err(store_err("wrong_type")),
        };
        if list.len() > MAX_PROFILES {
            return Err(store_err("too_many_profiles"));
        }
        for entry in list {
            let e = obj(entry, &["adopted", "author", "branch", "profile", "ref"])?;
            let doc = profile_from_value(e.get("profile").ok_or_else(|| store_err("bad_keys"))?, vocab)?;
            let r = ProfileRef::parse(str_of(e, "ref")?).ok_or_else(|| store_err("bad_ref"))?;
            if r != doc.reference() {
                return Err(store_err("digest_mismatch"));
            }
            let branch = Branch::parse(str_of(e, "branch")?).ok_or_else(|| store_err("bad_branch"))?;
            let author = parse_principal(str_of(e, "author")?).ok_or_else(|| store_err("bad_author"))?;
            let adopted = match e.get("adopted") {
                Some(CValue::Bool(b)) => *b,
                _ => return Err(store_err("wrong_type")),
            };
            // Working: a human's own entry, or an adopted agent profile. Agent: never adopted.
            let author_branch = Branch::for_principal(author);
            let consistent = match branch {
                Branch::Working => (author_branch == Branch::Agent) == adopted,
                Branch::Agent => author_branch == Branch::Agent && !adopted,
            };
            if !consistent {
                return Err(store_err("branch_author_mismatch"));
            }
            let id = doc.profile.id();
            if state.profiles.insert((branch, id), StoredProfile { doc, branch, author, adopted }).is_some() {
                return Err(store_err("duplicate_profile"));
            }
        }
        let bindings = match top.get("bindings") {
            Some(b) => obj(b, &["agent", "working"])?,
            None => return Err(store_err("bad_keys")),
        };
        for branch in [Branch::Working, Branch::Agent] {
            let side = match bindings.get(branch.as_str()) {
                Some(CValue::Obj(m)) => m,
                _ => return Err(store_err("wrong_type")),
            };
            if side.len() > MAX_BINDINGS {
                return Err(store_err("too_many_bindings"));
            }
            for (c, r) in side {
                let c = CharId::parse(c).ok_or_else(|| store_err("bad_char_id"))?;
                let r = match r {
                    CValue::Str(t) => ProfileRef::parse(t).ok_or_else(|| store_err("bad_ref"))?,
                    _ => return Err(store_err("wrong_type")),
                };
                if state.resolve(branch, &r).is_err() {
                    return Err(store_err("binding_unresolved"));
                }
                match branch {
                    Branch::Working => state.working.insert(c, r),
                    Branch::Agent => state.agent.insert(c, r),
                };
            }
        }
        Ok(state)
    }
}

fn obj<'a>(v: &'a CValue, want: &[&str]) -> Result<&'a BTreeMap<String, CValue>, DprofError> {
    match v {
        CValue::Obj(o) if o.len() == want.len() && want.iter().all(|k| o.contains_key(*k)) => Ok(o),
        CValue::Obj(_) => Err(store_err("bad_keys")),
        _ => Err(store_err("wrong_type")),
    }
}

fn str_of<'a>(o: &'a BTreeMap<String, CValue>, k: &str) -> Result<&'a str, DprofError> {
    match o.get(k) {
        Some(CValue::Str(s)) => Ok(s),
        _ => Err(store_err("wrong_type")),
    }
}
