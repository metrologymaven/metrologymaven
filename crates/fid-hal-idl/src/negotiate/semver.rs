//! Semver rules for capability packages (REQ-HAL.IDL-007, REQ-HAL.IDL.NEG-004).
//!
//! - A **breaking** change moves `M.m.p` to `(M+1).0.0`, an **additive** change to
//!   `M.(m+1).0`, a **fix** to `M.m.(p+1)`. Package versions here are plain `MAJOR.MINOR.PATCH`;
//!   pre-release and build suffixes are refused, so a released package never carries one.
//! - From one release's support table to the next, the previous current major of every
//!   capability stays negotiable (it may become deprecated, never vanish), and no capability
//!   disappears. A major already deprecated in the previous release may be dropped.

use super::{NegError, NegErrorKind, RuntimeSupport};
use crate::package::is_semver;

/// The kind of change made to a capability package.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Change {
    /// Removes or changes something a driver or consumer relies on.
    Breaking,
    /// Adds an operation, field or feature, keeping every existing one.
    Additive,
    /// Corrects behaviour without changing the contract.
    Fix,
}

fn bump_err(entry: &str, reason: &'static str) -> NegError {
    NegError::new(NegErrorKind::VersionBumpInvalid, entry, reason)
}

fn plain(s: &str, entry: &str) -> Result<(u64, u64, u64), NegError> {
    if !is_semver(s) || s.contains(['-', '+']) {
        return Err(bump_err(entry, "version_not_plain_semver"));
    }
    let mut parts = s.split('.').map(str::parse::<u64>);
    match (parts.next(), parts.next(), parts.next()) {
        (Some(Ok(a)), Some(Ok(b)), Some(Ok(c))) => Ok((a, b, c)),
        _ => Err(bump_err(entry, "version_not_plain_semver")),
    }
}

/// Checks that `new` is the version a `change` to a package at `old` must get.
pub fn check_version_bump(old: &str, new: &str, change: Change) -> Result<(), NegError> {
    let (ma, mi, pa) = plain(old, "old")?;
    let got = plain(new, "new")?;
    let want = match change {
        Change::Breaking => ma.checked_add(1).map(|m| (m, 0, 0)),
        Change::Additive => mi.checked_add(1).map(|m| (ma, m, 0)),
        Change::Fix => pa.checked_add(1).map(|p| (ma, mi, p)),
    };
    if want == Some(got) {
        Ok(())
    } else {
        Err(bump_err("new", "wrong_bump"))
    }
}

/// Checks the support table of the next release against the previous one: every capability
/// stays, and its previous current (highest, not deprecated) major stays negotiable. Every
/// major present in both tables keeps at least its highest minor and every feature it had,
/// because removing either inside a major is a breaking change.
/// [`RuntimeSupport::new`] already requires that a superseded major is marked deprecated.
pub fn check_major_retention(previous: &RuntimeSupport, next: &RuntimeSupport) -> Result<(), NegError> {
    for (name, majors) in previous.capabilities() {
        let Some(next_majors) = next.capabilities().get(name) else {
            return Err(NegError::support(name.clone(), "capability_dropped"));
        };
        if let Some(current) = majors.keys().next_back() {
            if !next_majors.contains_key(current) {
                return Err(NegError::support(name.clone(), "previous_major_dropped"));
            }
        }
        for (major, prev) in majors {
            let Some(next_row) = next_majors.get(major) else {
                continue;
            };
            if next_row.highest_minor < prev.highest_minor {
                return Err(NegError::support(name.clone(), "minor_regressed"));
            }
            if !prev.features.is_subset(&next_row.features) {
                return Err(NegError::support(name.clone(), "feature_dropped"));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn bumps_follow_semver() {
        // Hand-labelled from the rules in the module documentation.
        assert_eq!(check_version_bump("1.4.2", "2.0.0", Change::Breaking), Ok(()));
        assert_eq!(check_version_bump("1.4.2", "1.5.0", Change::Additive), Ok(()));
        assert_eq!(check_version_bump("1.4.2", "1.4.3", Change::Fix), Ok(()));
        for (old, new, change) in [
            ("1.4.2", "1.5.0", Change::Breaking),
            ("1.4.2", "2.1.0", Change::Breaking),
            ("1.4.2", "3.0.0", Change::Breaking),
            ("1.4.2", "2.0.0", Change::Additive),
            ("1.4.2", "1.5.2", Change::Additive),
            ("1.4.2", "1.4.3", Change::Additive),
            ("1.4.2", "1.5.0", Change::Fix),
            ("1.4.2", "1.4.2", Change::Fix),
        ] {
            assert_eq!(check_version_bump(old, new, change).map_err(|e| e.reason), Err("wrong_bump"), "{old} {new}");
        }
        for (old, new) in [("1.4", "1.5.0"), ("1.4.2", "1.5.0-rc.1"), ("1.4.2+b", "1.4.3"), ("01.4.2", "1.4.3")] {
            let e = check_version_bump(old, new, Change::Fix).unwrap_err();
            assert_eq!((e.code(), e.reason), ("version_bump_invalid", "version_not_plain_semver"), "{old} {new}");
        }
    }
}
