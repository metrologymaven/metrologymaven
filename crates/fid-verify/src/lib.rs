//! Standalone offline verifier for Metrology Maven Result Packages
//! (spec nodes `TRUST.VERIFY.CLI` and `TRUST.VERIFY.REPORT`).
//!
//! Pure Rust, read-only, deterministic (D0), with zero network, licence, account,
//! telemetry, SQLite, bus, store, or GUI dependencies. Both the `fid-verify` CLI binary
//! and the `verify.package@1` registry command share [`runner::verify_package`].

pub mod command;
pub mod report;
pub mod runner;

pub use report::{
    ChainSection, ReplayKernelInfo, ReplayResultEntry, ReplaySection, ReplayStatus, SectionStatus, SignaturesSection,
    ToolInfo, Verdict, VerificationReport, EXIT_INTERNAL, EXIT_MALFORMED, EXIT_PASS, EXIT_USAGE, FAIL_BIT_CHAIN,
    FAIL_BIT_REPLAY_DIVERGENT, FAIL_BIT_REPLAY_NOT_REPLAYABLE, FAIL_BIT_SIG, REPORT_VERSION, SUPPORTED_PACKAGE_FORMATS,
    TOOL_NAME, TOOL_VERSION,
};
pub use runner::{verify_package, VerifyError, VerifyErrorKind, VerifyOptions};
