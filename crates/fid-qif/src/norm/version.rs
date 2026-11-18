//! The version check of the rule file (REQ-IO.QIF.NORM-002, AC-5).
//!
//! `qif-norm.lock` records, for every released version of `qif-norm.toml`, the SHA-256
//! of the file (line ends normalised to LF), one `<version> <sha256:digest>` line per
//! version, oldest first. Lines that are empty or start with `#` are ignored. The check:
//!
//! 1. the lock is well formed and its version numbers strictly increase
//!    (`norm_lock_invalid`);
//! 2. the version the rule file declares is the last line of the lock
//!    (`norm_lock_stale`: the version was bumped but the lock not extended);
//! 3. the digest of the rule file equals the digest on that line
//!    (`norm_version_not_bumped`: the rules changed but the version did not).
//!
//! The test `ac5_shipped_rule_file_matches_lock` runs it on the shipped files in CI.
//! Editing an existing lock line to match changed rules defeats it; reviewers treat lock
//! lines as append-only.
//!
//! Follow-up (open question in the IO.QIF.NORM node): a CI step should also compare the
//! rule file and lock against the PR's base branch, failing `norm_version_not_bumped`
//! when the rule file changed and no lock line was appended, and when an existing lock
//! line was edited. That needs a workflow change, which goes in its own PR.

use super::rules::parse_version;
use super::{NormError, NormRules};
use fid_types::Digest;
use sha2::{Digest as _, Sha256};

/// Checks `rules_text` (a rule file) against `lock_text` (a lock file).
pub fn check_version(rules_text: &str, lock_text: &str) -> Result<(), NormError> {
    let bad = |reason| NormError::LockInvalid { reason };
    let mut entries: Vec<(String, u32, Digest)> = Vec::new();
    for line in lock_text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split_whitespace();
        let (Some(version), Some(digest), None) = (parts.next(), parts.next(), parts.next()) else {
            return Err(bad("a line is not `<version> <digest>`"));
        };
        let number = parse_version(version).ok_or(bad("a version is not qif-norm@<n>"))?;
        let digest = Digest::parse(digest).map_err(|_| bad("a digest is not sha256:<64 hex>"))?;
        if entries.last().is_some_and(|(_, prev, _)| *prev >= number) {
            return Err(bad("versions do not strictly increase"));
        }
        entries.push((version.to_string(), number, digest));
    }
    let rules = NormRules::parse(rules_text)?;
    let Some((last, _, locked)) = entries.last() else {
        return Err(bad("the lock has no versions"));
    };
    if last != rules.version() {
        return Err(NormError::LockStale { version: rules.version().to_string() });
    }
    if rule_file_digest(rules_text) != *locked {
        return Err(NormError::VersionNotBumped { version: rules.version().to_string() });
    }
    Ok(())
}

/// SHA-256 of the rule file with CRLF line ends replaced by LF.
pub fn rule_file_digest(rules_text: &str) -> Digest {
    let normalised = rules_text.replace("\r\n", "\n");
    let bytes: [u8; 32] = Sha256::digest(normalised.as_bytes()).into();
    Digest::from(bytes)
}
