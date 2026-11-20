//! Register verification and the status lookup (REQ-GDT.RES.STATUS-007, -008, -010).

use super::inputs::{CoiInputs, JournalExtract, Published};
use super::register::{build_register, digest_of, BuiltFrom, RuleStatusEntry, RuleStatusRegister};
use super::{Purpose, Reason, Status};
use crate::dmt_flow::roster::Roster;
use fid_types::Digest;

/// Why a register did not verify.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Unverified {
    /// `register_mismatch`: the file is not byte-identical to the register rebuilt from the
    /// journal (or cannot be read, or cannot be rebuilt).
    RegisterMismatch,
    /// `register_not_published`: its digest is not in `gdt-published/v1`.
    RegisterNotPublished,
    /// `foreign_roster`: its roster digest, or a roster given, is not listed there, or the
    /// listed register names another roster.
    ForeignRoster,
    /// `stale_register`: listed, but not the latest, and the purpose is `evaluate`.
    StaleRegister,
}

impl Unverified {
    /// Wire form.
    pub fn as_str(self) -> &'static str {
        match self {
            Unverified::RegisterMismatch => "register_mismatch",
            Unverified::RegisterNotPublished => "register_not_published",
            Unverified::ForeignRoster => "foreign_roster",
            Unverified::StaleRegister => "stale_register",
        }
    }
}

/// The outcome of [`verify_register`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verification {
    /// `verified`, with the register digest.
    Verified {
        /// SHA-256 of the register file.
        digest: Digest,
    },
    /// `unverified`, with the reason.
    Unverified {
        /// Why.
        reason: Unverified,
    },
}

impl Verification {
    /// `verified` or the reason code.
    pub fn code(self) -> &'static str {
        match self {
            Verification::Verified { .. } => "verified",
            Verification::Unverified { reason } => reason.as_str(),
        }
    }
}

/// A register file after [`verify_register`]: the outcome, and the register itself only when
/// it verified. There is no other constructor, so [`status_of`] can only read a register that
/// was rebuilt from the journal and matched against the published digests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckedRegister {
    verification: Verification,
    purpose: Purpose,
    register: Option<RuleStatusRegister>,
}

impl CheckedRegister {
    /// The outcome.
    pub fn verification(&self) -> Verification {
        self.verification
    }

    /// The purpose it was checked for.
    pub fn purpose(&self) -> Purpose {
        self.purpose
    }

    /// The register, only when it verified.
    pub fn register(&self) -> Option<&RuleStatusRegister> {
        self.register.as_ref()
    }
}

/// Verifies a register file (REQ-007, REQ-010). In order:
///
/// 1. the file's `built_from` must be readable (`register_mismatch` otherwise);
/// 2. its roster digest must be listed in `published` (`foreign_roster`);
/// 3. `rosters` must hold that roster and every trusted roster the file records
///    (`register_mismatch`), and they and the recorded ones must all be listed
///    (`foreign_roster`);
/// 4. the register rebuilt from `journal` up to the file's `journal_head`, with those rosters
///    and `coi`, must be byte-identical to the file (`register_mismatch`);
/// 5. the file's digest must be listed (`register_not_published`), paired with the same
///    roster digest (`foreign_roster`);
/// 6. for [`Purpose::Evaluate`] it must be the last entry (`stale_register`).
pub fn verify_register(
    file: &[u8],
    journal: &JournalExtract,
    rosters: &[Roster],
    coi: &CoiInputs,
    published: &Published,
    purpose: Purpose,
) -> CheckedRegister {
    let fail = |reason| CheckedRegister { verification: Verification::Unverified { reason }, purpose, register: None };
    let Ok(built) = BuiltFrom::of_file(file) else { return fail(Unverified::RegisterMismatch) };
    if !published.lists_roster(built.roster_digest) {
        return fail(Unverified::ForeignRoster);
    }
    let Some(current) = rosters.iter().find(|r| r.digest() == built.roster_digest) else {
        return fail(Unverified::RegisterMismatch);
    };
    if rosters.iter().any(|r| !published.lists_roster(r.digest()))
        || built.trusted_rosters.iter().any(|d| !published.lists_roster(*d))
    {
        return fail(Unverified::ForeignRoster);
    }
    // The rebuild uses exactly the trusted rosters the file records; each must be given.
    let mut chosen = Vec::with_capacity(built.trusted_rosters.len());
    for d in &built.trusted_rosters {
        match rosters.iter().find(|r| r.digest() == *d) {
            Some(r) => chosen.push(r.clone()),
            None => return fail(Unverified::RegisterMismatch),
        }
    }
    let Ok(trusted) = published.trusted_rosters(chosen) else { return fail(Unverified::ForeignRoster) };
    let Ok(rebuilt) = build_register(journal, Some(built.journal_head), current, &trusted, coi) else {
        return fail(Unverified::RegisterMismatch);
    };
    match rebuilt.canonical_bytes() {
        Ok(b) if b == file => {}
        _ => return fail(Unverified::RegisterMismatch),
    }
    let digest = digest_of(file);
    let Some((i, entry)) = published.find(digest) else { return fail(Unverified::RegisterNotPublished) };
    if entry.roster_digest != built.roster_digest {
        return fail(Unverified::ForeignRoster);
    }
    if purpose == Purpose::Evaluate && i + 1 != published.entries().len() {
        return fail(Unverified::StaleRegister);
    }
    CheckedRegister { verification: Verification::Verified { digest }, purpose, register: Some(rebuilt) }
}

/// The answer of [`status_of`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatusAnswer<'a> {
    /// The status.
    pub status: Status,
    /// The reason, if any.
    pub reason: Option<Reason>,
    /// The register entry answered from, if any.
    pub entry: Option<&'a RuleStatusEntry>,
}

impl StatusAnswer<'_> {
    fn provisional(reason: Reason) -> StatusAnswer<'static> {
        StatusAnswer { status: Status::Provisional, reason: Some(reason), entry: None }
    }
}

/// The status of `rule` at `content_digest` (REQ-008): `provisional`/`no_register` without a
/// register, `provisional`/`unverified_register` when it did not verify,
/// `provisional`/`not_in_register` when the verified register has no entry for that rule and
/// digest, and otherwise the entry's status and reason (the latest revision with that digest).
/// It never answers `signed_off` without a matching entry in a verified register.
pub fn status_of<'a>(register: Option<&'a CheckedRegister>, rule: &str, content_digest: Digest) -> StatusAnswer<'a> {
    let Some(checked) = register else { return StatusAnswer::provisional(Reason::NoRegister) };
    let Some(reg) = checked.register() else { return StatusAnswer::provisional(Reason::UnverifiedRegister) };
    match reg.entries().iter().rev().find(|e| e.rule == rule && e.content_digest == content_digest) {
        None => StatusAnswer::provisional(Reason::NotInRegister),
        Some(e) => StatusAnswer { status: e.status, reason: e.reason, entry: Some(e) },
    }
}
