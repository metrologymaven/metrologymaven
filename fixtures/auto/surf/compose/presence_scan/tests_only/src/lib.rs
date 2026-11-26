//! Shipped code that names `HumanPresence` only in comments, strings and a test module. The
//! scan must report nothing: "impl HumanPresence for Fake" in a string is not code.

pub const NOTE: &str = "impl HumanPresence for Fake {}";

// impl HumanPresence for Commented {}

/* impl HumanPresence for Blocked {} */

#[cfg(test)]
mod tests {
    use fid_bus::principal::{HumanPresence, PresenceOutcome, UpgradeRequest};
    use std::time::Duration;

    struct AutoYes;

    impl HumanPresence for AutoYes {
        fn confirm(&self, req: &UpgradeRequest, _deadline: Duration) -> PresenceOutcome {
            PresenceOutcome::Confirmed { os_user: req.os_user.clone(), method: fid_journal::ConfirmMethod::OsPresence }
        }
    }
}
