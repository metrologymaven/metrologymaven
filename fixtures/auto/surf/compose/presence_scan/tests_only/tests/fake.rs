//! A test-only presence provider. Tests are never shipped, so the scan must not report it.

use fid_bus::principal::{HumanPresence, PresenceOutcome, UpgradeRequest};
use std::time::Duration;

struct AutoYes;

impl HumanPresence for AutoYes {
    fn confirm(&self, req: &UpgradeRequest, _deadline: Duration) -> PresenceOutcome {
        PresenceOutcome::Confirmed { os_user: req.os_user.clone(), method: fid_journal::ConfirmMethod::OsPresence }
    }
}
