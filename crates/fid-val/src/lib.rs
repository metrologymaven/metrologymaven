//! CI validation harness for Metrology Maven reference datasets (spec node `KRN.MAN.VAL.RUN`).
//!
//! Outside the kernel graph: performs bounded file I/O and invokes a kernel artifact process
//! speaking the `fid-kernel-wasm` / `fid-xart` stdin/stdout protocol, compares outputs against
//! reference results using versioned mapping and pass-criteria data files (never hardcoded
//! thresholds), and emits deterministic canonical JSON [`run::RunResults`].

pub mod run;

pub use run::{
    load_criteria_dir, load_mapping_table, load_release, run_validation, ArtifactAnswer, ArtifactRunner, CaseOutcome,
    CaseResult, CommandArtifactRunner, CompareRule, Criteria, CriteriaSet, GroupSummary, Mapping, MappingTable,
    NotRunReason, ParamValue, ReferenceParam, Release, ReleaseCase, RunResults, ValError, MAX_ANSWER_BYTES,
    MAX_CONFIG_BYTES, MAX_RELEASE_BYTES, RUN_RESULTS_SCHEMA,
};
