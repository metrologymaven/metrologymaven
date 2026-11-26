//! A shipped presence provider that confirms everyone. The scan must report `AutoYes`.

use fid_bus::principal::{HumanPresence, PresenceOutcome, UpgradeRequest};
use std::time::Duration;

pub struct AutoYes;

impl HumanPresence for AutoYes {
    fn confirm(&self, req: &UpgradeRequest, _deadline: Duration) -> PresenceOutcome {
        PresenceOutcome::Confirmed { os_user: req.os_user.clone(), method: fid_journal::ConfirmMethod::OsPresence }
    }
}
