//! `fid-knp1-lint`: the CI tool that enforces the KNP-1 numeric profile on the kernel
//! dependency closure (spec node `KRN.NUM.LINT`). Tooling layer; not a registry command and
//! not part of the kernel. Modules land one spec leaf at a time; see `spec/krn/num/lint/`.
//!
//! - [`findings`] (alias [`find`]): rule catalogue, findings, report and waivers (`KRN.NUM.LINT.FIND`).
//! - [`graph`]: kernel dependency closure, allowlist and licence check (`KRN.NUM.LINT.GRAPH`).
//! - [`flags`]: kernel build-configuration and emitted-code FMA check (`KRN.NUM.LINT.FLAGS`).
//! - [`data`]: reference-dataset denylist matcher (`KRN.NUM.LINT.DATA`).
//! - [`syn_scan`]: syntax-level banned-API scan over the kernel closure (`KRN.NUM.LINT.SYN`).

pub mod data;
pub mod findings;
pub mod flags;
pub mod graph;
pub mod syn_scan;

/// Short alias of [`findings`] (the leaf `KRN.NUM.LINT.FIND`).
pub use findings as find;

pub use findings::{
    apply_waivers, catalogue, input_digest, lines_digest, load_waivers, parse_waivers, sort_findings, Applied,
    CrateRoots, ErrorCode, FindError, Finding, InputDigests, Report, Rule, SourceTree, Waiver, Warning, REPORT_SCHEMA,
    TOOL_NAME, WAIVER_UNUSED,
};
